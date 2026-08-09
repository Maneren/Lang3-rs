use std::{
    io::{BufRead as _, Write as _},
    rc::Rc,
    slice, thread,
    time::Duration,
};

use l3_runtime::{error::RuntimeResult, *};
use rand::RngExt as _;

type Builtin = Rc<dyn Fn(&[StackValue], &mut Heap) -> RuntimeResult<StackValue>>;

fn wrap(f: fn(&[StackValue], &mut Heap) -> RuntimeResult<StackValue>) -> Builtin {
    Rc::new(f)
}

fn wrap_infallible(f: fn(&[StackValue], &mut Heap) -> StackValue) -> Builtin {
    Rc::new(move |args, heap| Ok(f(args, heap)))
}

fn heap_data<'a>(heap: &'a Heap, sv: &StackValue) -> RuntimeResult<&'a HeapData> {
    match sv {
        StackValue::Heap(key) => heap
            .cells
            .get(*key)
            .map(|c| &c.value)
            .ok_or_else(|| RuntimeError::type_error("invalid heap reference")),
        _ => Err(RuntimeError::type_error("expected a heap value")),
    }
}

const fn int_val(sv: &StackValue) -> Option<i64> {
    match sv.as_primitive() {
        Some(Primitive::Integer(i)) => Some(i),
        _ => None,
    }
}

fn extract_vector(heap: &Heap, sv: &StackValue) -> RuntimeResult<Vec<StackValue>> {
    match heap_data(heap, sv)? {
        HeapData::Vector(v) => Ok(v.clone()),
        _ => Err(RuntimeError::type_error("expected a vector")),
    }
}

fn extract_fn(heap: &Heap, sv: &StackValue) -> RuntimeResult<HeapData> {
    match heap_data(heap, sv)? {
        data @ HeapData::Function(_) => Ok(data.clone()),
        _ => Err(RuntimeError::type_error("expected a function")),
    }
}

fn write_output(args: &[StackValue], heap: &mut Heap, newline: bool) -> RuntimeResult<()> {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            write!(heap.output, " ")?;
        }
        write!(heap.output, "{}", format_stack_value(arg, heap))?;
    }
    if newline {
        writeln!(heap.output)?;
    }
    Ok(())
}

fn builtin_print(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    write_output(args, heap, false)?;
    Ok(StackValue::Nil)
}

fn builtin_println(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    write_output(args, heap, true)?;
    Ok(StackValue::Nil)
}

fn builtin_assert(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let Some(condition) = args.first() else {
        return Err(RuntimeError::type_error(
            "assert requires at least 1 argument",
        ));
    };
    if !condition.is_truthy(heap) {
        let msg = args.get(1).map_or_else(
            || "assertion failed".to_string(),
            |message| format_stack_value(message, heap),
        );
        let err_msg = format!("AssertionError: {msg}");
        writeln!(heap.output, "{err_msg}")?;
        return Err(RuntimeError::value(format!("assertion failed: {msg}")));
    }
    Ok(StackValue::Nil)
}

fn builtin_error(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let msg = args.first().map_or_else(
        || "error".to_string(),
        |arg0| format_stack_value(arg0, heap),
    );
    let err_msg = format!("Error: {msg}");
    writeln!(heap.output, "{err_msg}")?;
    Err(RuntimeError::value(format!("error: {msg}")))
}

fn builtin_int(args: &[StackValue], heap: &mut Heap) -> StackValue {
    match args.first() {
        Some(StackValue::Primitive(Primitive::Integer(i))) => {
            StackValue::Primitive(Primitive::Integer(*i))
        },
        Some(StackValue::Primitive(Primitive::Double(f))) => {
            StackValue::Primitive(Primitive::Integer(*f as i64))
        },
        Some(StackValue::Primitive(Primitive::Bool(b))) => {
            StackValue::Primitive(Primitive::Integer(i64::from(*b)))
        },
        Some(StackValue::Heap(key)) => {
            if let Some(cell) = heap.cells.get(*key)
                && let Some(s) = cell.value.as_string()
                && let Ok(n) = s.parse::<i64>()
            {
                return StackValue::Primitive(Primitive::Integer(n));
            }
            StackValue::Primitive(Primitive::Integer(0))
        },
        Some(StackValue::Nil) | None => StackValue::Primitive(Primitive::Integer(0)),
    }
}

fn builtin_str(args: &[StackValue], heap: &mut Heap) -> StackValue {
    let text = args
        .first()
        .map_or_else(String::new, |value| format_stack_value(value, heap));
    heap.alloc_string(text)
}

fn builtin_len(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let Some(container) = args.first() else {
        return Err(RuntimeError::type_error("len requires an argument"));
    };
    let len = match heap_data(heap, container) {
        Ok(HeapData::String(s)) => s.chars().count() as i64,
        Ok(HeapData::Vector(v)) => v.len() as i64,
        _ => {
            return Err(RuntimeError::type_error(format!(
                "len requires a string or vector, got {}",
                container.type_name(heap)
            )));
        },
    };
    Ok(StackValue::Primitive(Primitive::Integer(len)))
}

fn builtin_head(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let Some(container) = args.first() else {
        return Err(RuntimeError::type_error("head requires an argument"));
    };
    match heap_data(heap, container)? {
        HeapData::Vector(v) => Ok(*v
            .first()
            .ok_or_else(|| RuntimeError::value("head of empty vector"))?),
        HeapData::String(s) => s.chars().next().map_or_else(
            || Err(RuntimeError::value("head of empty string")),
            |c| Ok(heap.alloc_string(c.to_string())),
        ),
        _ => Err(RuntimeError::type_error("head requires a vector or string")),
    }
}

fn builtin_tail(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let Some(container) = args.first() else {
        return Err(RuntimeError::type_error("tail requires an argument"));
    };
    match heap_data(heap, container)? {
        HeapData::Vector(v) => Ok(heap.alloc_vector(
            v.get(1..)
                .ok_or_else(|| RuntimeError::value("tail of empty vector"))?
                .to_vec(),
        )),
        HeapData::String(s) => {
            if s.is_empty() {
                return Err(RuntimeError::value("tail of empty string"));
            }
            Ok(heap.alloc_string(s.chars().skip(1).collect()))
        },
        _ => Err(RuntimeError::type_error("tail requires a vector or string")),
    }
}

fn builtin_drop(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let Some(container) = args.first() else {
        return Err(RuntimeError::type_error("drop requires 2 arguments"));
    };
    let Some(count) = args.get(1) else {
        return Err(RuntimeError::type_error("drop requires 2 arguments"));
    };
    let Some(count) = int_val(count) else {
        return Err(RuntimeError::type_error("drop requires integer count"));
    };
    let n = usize::try_from(count).unwrap_or(usize::MAX);
    match heap_data(heap, container)? {
        HeapData::Vector(v) => Ok(heap.alloc_vector(v.get(n..).unwrap_or_default().to_vec())),
        HeapData::String(s) => Ok(heap.alloc_string(s.chars().skip(n).collect())),
        _ => Err(RuntimeError::type_error("drop requires a vector or string")),
    }
}

fn builtin_take(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let Some(container) = args.first() else {
        return Err(RuntimeError::type_error("take requires 2 arguments"));
    };
    let Some(count) = args.get(1) else {
        return Err(RuntimeError::type_error("take requires 2 arguments"));
    };
    let Some(count) = int_val(count) else {
        return Err(RuntimeError::type_error("take requires integer count"));
    };
    let n = usize::try_from(count).unwrap_or(usize::MAX);
    match heap_data(heap, container)? {
        HeapData::Vector(v) => {
            let end = n.min(v.len());
            Ok(heap.alloc_vector(v.get(..end).unwrap_or_default().to_vec()))
        },
        HeapData::String(s) => Ok(heap.alloc_string(s.chars().take(n).collect())),
        _ => Err(RuntimeError::type_error("take requires a vector or string")),
    }
}

fn builtin_range(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let start = args.first().map_or(Ok(0), |a| {
        int_val(a).ok_or_else(|| RuntimeError::type_error("range requires integer arguments"))
    })?;
    let end = args.get(1).map_or(Ok(start), |a| {
        int_val(a).ok_or_else(|| RuntimeError::type_error("range requires integer arguments"))
    })?;
    let step = args.get(2).map_or(Ok(1), |a| match int_val(a) {
        Some(i) if i != 0 => Ok(i),
        _ => Err(RuntimeError::type_error(
            "range requires non-zero integer step",
        )),
    })?;

    let mut vec = Vec::new();
    let mut i = start;
    if step > 0 {
        while i < end {
            vec.push(StackValue::Primitive(Primitive::Integer(i)));
            i += step;
        }
    } else {
        while i > end {
            vec.push(StackValue::Primitive(Primitive::Integer(i)));
            i += step;
        }
    }
    Ok(heap.alloc_vector(vec))
}

fn builtin_id(args: &[StackValue], _heap: &mut Heap) -> RuntimeResult<StackValue> {
    args.first()
        .copied()
        .ok_or_else(|| RuntimeError::type_error("id requires an argument"))
}

fn builtin_map(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let func_data = extract_fn(
        heap,
        args.first()
            .ok_or_else(|| RuntimeError::type_error("map requires a function and a vector"))?,
    )?;
    let vec = extract_vector(
        heap,
        args.get(1)
            .ok_or_else(|| RuntimeError::type_error("map requires a function and a vector"))?,
    )?;
    match func_data {
        HeapData::Function(Function::Builtin(b)) => {
            let mut result = Vec::new();
            for elem in &vec {
                result.push(b.invoke(slice::from_ref(elem), heap)?);
            }
            Ok(heap.alloc_vector(result))
        },
        _ => Err(RuntimeError::type_error(
            "map currently only supports builtin functions",
        )),
    }
}

fn builtin_count(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let func_data = extract_fn(
        heap,
        args.first()
            .ok_or_else(|| RuntimeError::type_error("count requires a predicate and a vector"))?,
    )?;
    let vec = extract_vector(
        heap,
        args.get(1)
            .ok_or_else(|| RuntimeError::type_error("count requires a predicate and a vector"))?,
    )?;
    match func_data {
        HeapData::Function(Function::Builtin(b)) => {
            let mut total = 0i64;
            for elem in &vec {
                if b.invoke(slice::from_ref(elem), heap)?.is_truthy(heap) {
                    total += 1;
                }
            }
            Ok(StackValue::Primitive(Primitive::Integer(total)))
        },
        _ => Err(RuntimeError::type_error(
            "count currently only supports builtin functions",
        )),
    }
}

fn builtin_random(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let limit = args.first().map_or(Ok(i64::MAX), |a| match int_val(a) {
        Some(i) if i > 0 => Ok(i),
        _ => Err(RuntimeError::type_error(
            "random requires a positive integer argument",
        )),
    })?;
    let val = heap.rng.random_range(0..limit);
    Ok(StackValue::Primitive(Primitive::Integer(val)))
}

fn builtin_input(_args: &[StackValue], heap: &mut Heap) -> StackValue {
    let mut line = String::new();
    heap.input.read_line(&mut line).ok();
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    heap.alloc_string(line)
}

fn builtin_sleep(args: &[StackValue], _heap: &mut Heap) -> RuntimeResult<StackValue> {
    let ms = args.first().map_or(Ok(0), |a| {
        int_val(a)
            .and_then(|i| u64::try_from(i).ok())
            .ok_or_else(|| {
                RuntimeError::type_error("sleep requires a non-negative integer argument")
            })
    })?;
    thread::sleep(Duration::from_millis(ms));
    Ok(StackValue::Nil)
}

fn builtin_sum(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let Some(container) = args.first() else {
        return Err(RuntimeError::type_error("sum requires an argument"));
    };
    let mut total: f64 = 0.0;
    let mut is_int = true;
    let HeapData::Vector(v) = heap_data(heap, container).map_err(|_| {
        RuntimeError::type_error(format!(
            "sum requires a vector, got {}",
            container.type_name(heap)
        ))
    })?
    else {
        return Err(RuntimeError::type_error(format!(
            "sum requires a vector, got {}",
            container.type_name(heap)
        )));
    };
    for sv in v {
        match sv.as_primitive() {
            Some(Primitive::Integer(i)) => total += i as f64,
            Some(Primitive::Double(f)) => {
                total += f;
                is_int = false;
            },
            _ => {
                return Err(RuntimeError::type_error(format!(
                    "sum requires numeric elements, got {}",
                    sv.type_name(heap)
                )));
            },
        }
    }
    if is_int && total.fract() == 0.0 {
        Ok(StackValue::Primitive(Primitive::Integer(total as i64)))
    } else {
        Ok(StackValue::Primitive(Primitive::Double(total)))
    }
}

fn builtin_trigger_gc(_args: &[StackValue], heap: &mut Heap) -> StackValue {
    let erased = heap.sweep();
    heap.alloc_string(format!("GC swept {erased} cells"))
}

#[must_use]
pub fn builtins() -> Vec<(&'static str, Builtin)> {
    vec![
        ("print", wrap(builtin_print)),
        ("println", wrap(builtin_println)),
        ("assert", wrap(builtin_assert)),
        ("error", wrap(builtin_error)),
        ("int", wrap_infallible(builtin_int)),
        ("str", wrap_infallible(builtin_str)),
        ("len", wrap(builtin_len)),
        ("head", wrap(builtin_head)),
        ("tail", wrap(builtin_tail)),
        ("drop", wrap(builtin_drop)),
        ("take", wrap(builtin_take)),
        ("range", wrap(builtin_range)),
        ("id", wrap(builtin_id)),
        ("map", wrap(builtin_map)),
        ("count", wrap(builtin_count)),
        ("random", wrap(builtin_random)),
        ("input", wrap_infallible(builtin_input)),
        ("sleep", wrap(builtin_sleep)),
        ("sum", wrap(builtin_sum)),
        ("__trigger_gc", wrap_infallible(builtin_trigger_gc)),
    ]
}

#[must_use]
pub fn format_stack_value(sv: &StackValue, heap: &Heap) -> String {
    match sv {
        StackValue::Nil => "nil".to_string(),
        StackValue::Primitive(p) => format!("{p}"),
        StackValue::Heap(key) => heap.cells.get(*key).map_or_else(
            || "<dead>".to_string(),
            |cell| match &cell.value {
                HeapData::Nil => "nil".to_string(),
                HeapData::Primitive(p) => format!("{p}"),
                HeapData::Function(f) => match f {
                    Function::Builtin(b) => format!("<builtin {}>", b.name),
                    Function::Bytecode(bc) => format!("<fn {}>", bc.name),
                },
                HeapData::Vector(v) => {
                    let elems: Vec<String> =
                        v.iter().map(|sv| format_stack_value(sv, heap)).collect();
                    format!("[{}]", elems.join(", "))
                },
                HeapData::String(s) => s.clone(),
            },
        ),
    }
}
