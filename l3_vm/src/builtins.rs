use std::{
    io::{BufRead as _, Write as _},
    rc::Rc,
    slice, thread,
    time::Duration,
};

use l3_runtime::{
    BuiltinId,
    conv::{as_integer, non_negative_count, to_integer},
    error::RuntimeResult,
    heap_data::stringify_stack_value,
    *,
};
use rand::RngExt as _;

pub type Builtin = Rc<dyn Fn(&[StackValue], &mut Heap) -> RuntimeResult<StackValue>>;

fn wrap(f: fn(&[StackValue], &mut Heap) -> RuntimeResult<StackValue>) -> Builtin {
    Rc::new(f)
}

fn wrap_infallible(f: fn(&[StackValue], &mut Heap) -> StackValue) -> Builtin {
    Rc::new(move |args, heap| Ok(f(args, heap)))
}

fn heap_data<'a>(heap: &'a Heap, sv: &StackValue) -> Option<&'a HeapData> {
    #[expect(
        clippy::unwrap_in_result,
        reason = "invalid heap references are impossible for builtin args"
    )]
    match sv {
        StackValue::Heap(key) => Some(
            heap.cells
                .get(*key)
                .map(|c| &c.value)
                .expect("the heap reference is valid"),
        ),
        _ => None,
    }
}

fn extract_vector<'a>(heap: &'a Heap, sv: &StackValue) -> RuntimeResult<&'a Vec<StackValue>> {
    heap_data(heap, sv)
        .and_then(|d| d.as_vector())
        .ok_or_else(|| RuntimeError::type_error("expected a vector"))
}

fn extract_fn<'a>(heap: &'a Heap, sv: &StackValue) -> RuntimeResult<&'a Function> {
    heap_data(heap, sv)
        .and_then(|d| d.as_function())
        .ok_or_else(|| RuntimeError::type_error("expected a function"))
}

fn write_output(args: &[StackValue], heap: &mut Heap, newline: bool) -> RuntimeResult<()> {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            write!(heap.env.output, " ")?;
        }
        write!(heap.env.output, "{}", stringify_stack_value(arg, heap))?;
    }
    if newline {
        writeln!(heap.env.output)?;
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
            |message| stringify_stack_value(message, heap),
        );
        let err_msg = format!("AssertionError: {msg}");
        writeln!(heap.env.output, "{err_msg}")?;
        return Err(RuntimeError::value(format!("assertion failed: {msg}")));
    }
    Ok(StackValue::Nil)
}

fn builtin_error(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let msg = args.first().map_or_else(
        || "error".to_string(),
        |arg0| stringify_stack_value(arg0, heap),
    );
    let err_msg = format!("Error: {msg}");
    writeln!(heap.env.output, "{err_msg}")?;
    Err(RuntimeError::value(format!("error: {msg}")))
}

fn builtin_int(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let Some(value) = args.first() else {
        return Ok(StackValue::Primitive(Primitive::Integer(0)));
    };
    Ok(StackValue::Primitive(Primitive::Integer(to_integer(
        value, heap,
    )?)))
}

fn builtin_str(args: &[StackValue], heap: &mut Heap) -> StackValue {
    let text = args
        .first()
        .map_or_else(String::new, |value| stringify_stack_value(value, heap));
    heap.alloc_string(text)
}

fn builtin_len(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let Some(container) = args.first() else {
        return Err(RuntimeError::type_error("len requires an argument"));
    };
    let len = match heap_data(heap, container) {
        Some(HeapData::String(s)) => i64::try_from(s.chars().count())
            .map_err(|_e| RuntimeError::value("length of string is too large"))?,
        Some(HeapData::Vector(v)) => i64::try_from(v.len())
            .map_err(|_e| RuntimeError::value("length of vector is too large"))?,
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
    match heap_data(heap, container) {
        Some(HeapData::Vector(v)) => Ok(*v
            .first()
            .ok_or_else(|| RuntimeError::value("head of empty vector"))?),
        Some(HeapData::String(s)) => s.chars().next().map_or_else(
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
    match heap_data(heap, container) {
        Some(HeapData::Vector(v)) => Ok(heap.alloc_vector(
            v.get(1..)
                .ok_or_else(|| RuntimeError::value("tail of empty vector"))?
                .to_vec(),
        )),
        Some(HeapData::String(s)) => {
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
    let count = match args.get(1) {
        Some(value) => {
            let n = as_integer(value)
                .ok_or_else(|| RuntimeError::type_error("drop requires integer count"))?;
            non_negative_count(n)?
        },
        _ => 1,
    };
    match heap_data(heap, container) {
        Some(HeapData::Vector(v)) => {
            Ok(heap.alloc_vector(v.get(count..).unwrap_or_default().to_vec()))
        },
        Some(HeapData::String(s)) => Ok(heap.alloc_string(s.chars().skip(count).collect())),
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
    let count =
        as_integer(count).ok_or_else(|| RuntimeError::type_error("take requires integer count"))?;
    let count = non_negative_count(count)?;
    match heap_data(heap, container) {
        Some(HeapData::Vector(v)) => {
            let end = count.min(v.len());
            Ok(heap.alloc_vector(v.get(..end).unwrap_or_default().to_vec()))
        },
        Some(HeapData::String(s)) => Ok(heap.alloc_string(s.chars().take(count).collect())),
        _ => Err(RuntimeError::type_error("take requires a vector or string")),
    }
}

fn builtin_range(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let start = args.first().map_or(Ok(0), |a| {
        as_integer(a).ok_or_else(|| RuntimeError::type_error("range requires integer arguments"))
    })?;
    let end = args.get(1).map_or(Ok(start), |a| {
        as_integer(a).ok_or_else(|| RuntimeError::type_error("range requires integer arguments"))
    })?;
    let step = args.get(2).map_or(Ok(1), |a| match as_integer(a) {
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
    )?
    .clone();
    match func_data {
        Function::Builtin(b) => {
            let mut result = Vec::new();
            let b = b.clone();
            for elem in vec {
                result.push(b.invoke(slice::from_ref(&elem), heap)?);
            }
            Ok(heap.alloc_vector(result))
        },
        Function::Bytecode(_) => Err(RuntimeError::type_error(
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
    )?
    .clone();
    match func_data {
        Function::Builtin(b) => {
            let mut total = 0i64;
            let b = b.clone();
            for elem in vec {
                if b.invoke(slice::from_ref(&elem), heap)?.is_truthy(heap) {
                    total += 1;
                }
            }
            Ok(StackValue::Primitive(Primitive::Integer(total)))
        },
        Function::Bytecode(_) => Err(RuntimeError::type_error(
            "count currently only supports builtin functions",
        )),
    }
}

fn builtin_random(args: &[StackValue], heap: &mut Heap) -> RuntimeResult<StackValue> {
    let limit = args.first().map_or(Ok(i64::MAX), |a| match as_integer(a) {
        Some(i) if i > 0 => Ok(i),
        _ => Err(RuntimeError::type_error(
            "random requires a positive integer argument",
        )),
    })?;
    let val = heap.env.rng.random_range(0..limit);
    Ok(StackValue::Primitive(Primitive::Integer(val)))
}

fn builtin_input(_args: &[StackValue], heap: &mut Heap) -> StackValue {
    let mut line = String::new();
    let Ok(_) = heap.env.input.read_line(&mut line) else {
        return StackValue::Nil;
    };
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
        as_integer(a)
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
    let Some(HeapData::Vector(v)) = heap_data(heap, container) else {
        return Err(RuntimeError::type_error(format!(
            "sum requires a vector, got {}",
            container.type_name(heap)
        )));
    };
    let mut int_total: i64 = 0;
    let mut double_total: f64 = 0.0;
    let mut is_float = false;
    for sv in v {
        match sv.as_primitive() {
            Some(Primitive::Integer(i)) => {
                if is_float {
                    double_total += i as f64;
                } else {
                    int_total = int_total.wrapping_add(i);
                }
            },
            Some(Primitive::Double(f)) => {
                if !is_float {
                    double_total = int_total as f64;
                    is_float = true;
                }
                double_total += f;
            },
            _ => {
                return Err(RuntimeError::type_error(format!(
                    "sum requires numeric elements, got {}",
                    sv.type_name(heap)
                )));
            },
        }
    }
    if is_float {
        Ok(StackValue::Primitive(Primitive::Double(double_total)))
    } else {
        Ok(StackValue::Primitive(Primitive::Integer(int_total)))
    }
}

#[must_use]
pub fn builtins() -> Vec<(&'static str, Builtin)> {
    vec![
        ("print", wrap(builtin_print)),
        ("println", wrap(builtin_println)),
        ("assert", wrap(builtin_assert)),
        ("error", wrap(builtin_error)),
        ("int", wrap(builtin_int)),
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
    ]
}

#[must_use]
pub fn builtin_for_id(id: BuiltinId) -> Builtin {
    match id {
        BuiltinId::Print => wrap(builtin_print),
        BuiltinId::Println => wrap(builtin_println),
        BuiltinId::Assert => wrap(builtin_assert),
        BuiltinId::Error => wrap(builtin_error),
        BuiltinId::Int => wrap(builtin_int),
        BuiltinId::Str => wrap_infallible(builtin_str),
        BuiltinId::Len => wrap(builtin_len),
        BuiltinId::Head => wrap(builtin_head),
        BuiltinId::Tail => wrap(builtin_tail),
        BuiltinId::Drop => wrap(builtin_drop),
        BuiltinId::Take => wrap(builtin_take),
        BuiltinId::Range => wrap(builtin_range),
        BuiltinId::Id => wrap(builtin_id),
        BuiltinId::Map => wrap(builtin_map),
        BuiltinId::Count => wrap(builtin_count),
        BuiltinId::Random => wrap(builtin_random),
        BuiltinId::Input => wrap_infallible(builtin_input),
        BuiltinId::Sleep => wrap(builtin_sleep),
        BuiltinId::Sum => wrap(builtin_sum),
    }
}
