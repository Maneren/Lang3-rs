use l3_runtime::*;
use std::io::Write;
use std::rc::Rc;

type Builtin = Rc<dyn Fn(Vec<StackValue>, &mut Heap) -> Result<StackValue, RuntimeError>>;

fn wrap(f: fn(Vec<StackValue>, &mut Heap) -> Result<StackValue, RuntimeError>) -> Builtin {
    Rc::new(f)
}

fn heap_data<'a>(heap: &'a Heap, sv: &StackValue) -> Result<&'a HeapData, RuntimeError> {
    match sv {
        StackValue::Heap(key) => heap
            .cells
            .get(*key)
            .map(|c| &c.value)
            .ok_or_else(|| RuntimeError::type_error("invalid heap reference")),
        _ => Err(RuntimeError::type_error("expected a heap value")),
    }
}

fn int_val(sv: &StackValue) -> Option<i64> {
    match sv.as_primitive() {
        Some(Primitive::Integer(i)) => Some(i),
        _ => None,
    }
}

fn extract_vector(heap: &Heap, sv: &StackValue) -> Result<Vec<StackValue>, RuntimeError> {
    match heap_data(heap, sv)? {
        HeapData::Vector(v) => Ok(v.clone()),
        _ => Err(RuntimeError::type_error("expected a vector")),
    }
}

fn extract_fn(heap: &Heap, sv: &StackValue) -> Result<HeapData, RuntimeError> {
    match heap_data(heap, sv)? {
        data @ HeapData::Function(_) => Ok(data.clone()),
        _ => Err(RuntimeError::type_error("expected a function")),
    }
}

fn write_output(args: &[StackValue], heap: &mut Heap, newline: bool) {
    if heap.stream_output {
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                print!(" ");
            }
            print!("{}", format_stack_value(arg, heap));
        }
        if newline {
            println!();
        }
        std::io::stdout().flush().ok();
    } else {
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                heap.current_line.push(' ');
            }
            heap.current_line.push_str(&format_stack_value(arg, heap));
        }
        if newline {
            heap.output_lines
                .push(std::mem::take(&mut heap.current_line));
        }
    }
}

fn builtin_print(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    write_output(&args, heap, false);
    Ok(StackValue::Nil)
}

fn builtin_println(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    write_output(&args, heap, true);
    Ok(StackValue::Nil)
}

fn builtin_assert(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::type_error(
            "assert requires at least 1 argument",
        ));
    }
    if !args[0].is_truthy(heap) {
        let msg = if args.len() > 1 {
            format_stack_value(&args[1], heap)
        } else {
            "assertion failed".to_string()
        };
        let err_msg = format!("AssertionError: {msg}");
        if heap.stream_output {
            println!("{err_msg}");
        }
        heap.output_lines.push(err_msg);
        return Err(RuntimeError::value(format!("assertion failed: {msg}")));
    }
    Ok(StackValue::Nil)
}

fn builtin_error(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    let msg = if args.is_empty() {
        "error".to_string()
    } else {
        format_stack_value(&args[0], heap)
    };
    let err_msg = format!("Error: {msg}");
    if heap.stream_output {
        println!("{err_msg}");
    }
    heap.output_lines.push(err_msg);
    Err(RuntimeError::value(format!("error: {msg}")))
}

fn builtin_int(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    if args.is_empty() {
        return Ok(StackValue::Primitive(Primitive::Integer(0)));
    }
    match &args[0] {
        StackValue::Primitive(Primitive::Integer(i)) => {
            Ok(StackValue::Primitive(Primitive::Integer(*i)))
        }
        StackValue::Primitive(Primitive::Double(f)) => {
            Ok(StackValue::Primitive(Primitive::Integer(*f as i64)))
        }
        StackValue::Primitive(Primitive::Bool(b)) => {
            Ok(StackValue::Primitive(Primitive::Integer(i64::from(*b))))
        }
        StackValue::Heap(key) => {
            if let Some(cell) = heap.cells.get(*key)
                && let Some(s) = cell.value.as_string()
                && let Ok(n) = s.parse::<i64>()
            {
                return Ok(StackValue::Primitive(Primitive::Integer(n)));
            }
            Ok(StackValue::Primitive(Primitive::Integer(0)))
        }
        StackValue::Nil => Ok(StackValue::Primitive(Primitive::Integer(0))),
    }
}

fn builtin_str(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    Ok(if args.is_empty() {
        heap.alloc_string(String::new())
    } else {
        heap.alloc_string(format_stack_value(&args[0], heap))
    })
}

fn builtin_len(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    if args.is_empty() {
        return Ok(StackValue::Primitive(Primitive::Integer(0)));
    }
    match &args[0] {
        StackValue::Heap(key) => {
            if let Some(cell) = heap.cells.get(*key) {
                match &cell.value {
                    HeapData::String(s) => Ok(StackValue::Primitive(Primitive::Integer(
                        s.len() as i64,
                    ))),
                    HeapData::Vector(v) => Ok(StackValue::Primitive(Primitive::Integer(
                        v.len() as i64,
                    ))),
                    _ => Ok(StackValue::Primitive(Primitive::Integer(0))),
                }
            } else {
                Ok(StackValue::Primitive(Primitive::Integer(0)))
            }
        }
        _ => Ok(StackValue::Primitive(Primitive::Integer(0))),
    }
}

fn builtin_head(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::type_error("head requires an argument"));
    }
    match heap_data(heap, &args[0])? {
        HeapData::Vector(v) => {
            if v.is_empty() {
                return Err(RuntimeError::value("head of empty vector"));
            }
            Ok(v[0].clone())
        }
        HeapData::String(s) => {
            if s.is_empty() {
                return Err(RuntimeError::value("head of empty string"));
            }
            Ok(heap.alloc_string(s.chars().next().unwrap().to_string()))
        }
        _ => Err(RuntimeError::type_error("head requires a vector or string")),
    }
}

fn builtin_tail(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::type_error("tail requires an argument"));
    }
    match heap_data(heap, &args[0])? {
        HeapData::Vector(v) => {
            if v.is_empty() {
                return Err(RuntimeError::value("tail of empty vector"));
            }
            Ok(heap.alloc_vector(v[1..].to_vec()))
        }
        HeapData::String(s) => {
            if s.is_empty() {
                return Err(RuntimeError::value("tail of empty string"));
            }
            Ok(heap.alloc_string(s.chars().skip(1).collect()))
        }
        _ => Err(RuntimeError::type_error("tail requires a vector or string")),
    }
}

fn builtin_drop(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::type_error("drop requires 2 arguments"));
    }
    let n = match int_val(&args[1]) {
        Some(i) => i as usize,
        _ => return Err(RuntimeError::type_error("drop requires integer count")),
    };
    match heap_data(heap, &args[0])? {
        HeapData::Vector(v) => {
            if n >= v.len() {
                return Ok(heap.alloc_vector(Vec::new()));
            }
            Ok(heap.alloc_vector(v[n..].to_vec()))
        }
        HeapData::String(s) => Ok(heap.alloc_string(s.chars().skip(n).collect())),
        _ => Err(RuntimeError::type_error("drop requires a vector or string")),
    }
}

fn builtin_take(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::type_error("take requires 2 arguments"));
    }
    let n = match int_val(&args[1]) {
        Some(i) => i as usize,
        _ => return Err(RuntimeError::type_error("take requires integer count")),
    };
    match heap_data(heap, &args[0])? {
        HeapData::Vector(v) => {
            let end = n.min(v.len());
            Ok(heap.alloc_vector(v[..end].to_vec()))
        }
        HeapData::String(s) => Ok(heap.alloc_string(s.chars().take(n).collect())),
        _ => Err(RuntimeError::type_error("take requires a vector or string")),
    }
}

fn builtin_range(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    let start = if args.is_empty() {
        0
    } else {
        match int_val(&args[0]) {
            Some(i) => i,
            _ => return Err(RuntimeError::type_error("range requires integer arguments")),
        }
    };
    let end = if args.len() > 1 {
        match int_val(&args[1]) {
            Some(i) => i,
            _ => return Err(RuntimeError::type_error("range requires integer arguments")),
        }
    } else {
        start
    };
    let step = if args.len() > 2 {
        match int_val(&args[2]) {
            Some(i) if i != 0 => i,
            _ => return Err(RuntimeError::type_error("range requires non-zero integer step")),
        }
    } else {
        1
    };

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

fn builtin_id(args: Vec<StackValue>, _heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    Ok(args.into_iter().next().unwrap_or(StackValue::Nil))
}

fn builtin_map(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::type_error(
            "map requires a function and a vector",
        ));
    }
    let func_data = extract_fn(heap, &args[0])?;
    let vec = extract_vector(heap, &args[1])?;
    match func_data {
        HeapData::Function(Function::Builtin(b)) => {
            let mut result = Vec::new();
            for elem in &vec {
                result.push(b.invoke(vec![elem.clone()], heap)?);
            }
            Ok(heap.alloc_vector(result))
        }
        _ => Err(RuntimeError::type_error(
            "map currently only supports builtin functions",
        )),
    }
}

fn builtin_count(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::type_error(
            "count requires a predicate and a vector",
        ));
    }
    let func_data = extract_fn(heap, &args[0])?;
    let vec = extract_vector(heap, &args[1])?;
    match func_data {
        HeapData::Function(Function::Builtin(b)) => {
            let mut total = 0i64;
            for elem in &vec {
                if b.invoke(vec![elem.clone()], heap)?.is_truthy(heap) {
                    total += 1;
                }
            }
            Ok(StackValue::Primitive(Primitive::Integer(total)))
        }
        _ => Err(RuntimeError::type_error(
            "count currently only supports builtin functions",
        )),
    }
}

fn builtin_random(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    heap.rng_state = heap
        .rng_state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    let val = heap.rng_state;
    let limit = if args.is_empty() {
        u64::MAX
    } else {
        match int_val(&args[0]) {
            Some(i) if i > 0 => i as u64,
            _ => {
                return Err(RuntimeError::type_error(
                    "random requires a positive integer argument",
                ));
            }
        }
    };
    Ok(StackValue::Primitive(Primitive::Integer(
        (val % limit) as i64,
    )))
}

fn builtin_input(_args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    if let Some(line) = heap.input_queue.pop_front() {
        return Ok(heap.alloc_string(line));
    }
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(heap.alloc_string(line))
}

fn builtin_sleep(args: Vec<StackValue>, _heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    let ms = if args.is_empty() {
        0
    } else {
        match int_val(&args[0]) {
            Some(i) if i >= 0 => i as u64,
            _ => {
                return Err(RuntimeError::type_error(
                    "sleep requires a non-negative integer argument",
                ));
            }
        }
    };
    std::thread::sleep(std::time::Duration::from_millis(ms));
    Ok(StackValue::Nil)
}

fn builtin_sum(args: Vec<StackValue>, heap: &mut Heap) -> Result<StackValue, RuntimeError> {
    if args.is_empty() {
        return Ok(StackValue::Primitive(Primitive::Integer(0)));
    }
    let mut total: f64 = 0.0;
    let mut is_int = true;
    if let Ok(HeapData::Vector(v)) = heap_data(heap, &args[0]) {
        for sv in &*v {
            match sv.as_primitive() {
                Some(Primitive::Integer(i)) => total += i as f64,
                Some(Primitive::Double(f)) => {
                    total += f;
                    is_int = false;
                }
                _ => {}
            }
        }
    }
    Ok(if is_int && total.fract() == 0.0 {
        StackValue::Primitive(Primitive::Integer(total as i64))
    } else {
        StackValue::Primitive(Primitive::Double(total))
    })
}

fn builtin_trigger_gc(
    _args: Vec<StackValue>,
    heap: &mut Heap,
) -> Result<StackValue, RuntimeError> {
    let erased = heap.sweep();
    Ok(heap.alloc_string(format!("GC swept {erased} cells")))
}

#[must_use]
pub fn builtins() -> Vec<(&'static str, Builtin)> {
    vec![
        ("print", wrap(builtin_print)),
        ("println", wrap(builtin_println)),
        ("assert", wrap(builtin_assert)),
        ("error", wrap(builtin_error)),
        ("int", wrap(builtin_int)),
        ("str", wrap(builtin_str)),
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
        ("input", wrap(builtin_input)),
        ("sleep", wrap(builtin_sleep)),
        ("sum", wrap(builtin_sum)),
        ("__trigger_gc", wrap(builtin_trigger_gc)),
    ]
}

#[must_use]
pub fn format_stack_value(sv: &StackValue, heap: &Heap) -> String {
    match sv {
        StackValue::Nil => "nil".to_string(),
        StackValue::Primitive(p) => format!("{p}"),
        StackValue::Heap(key) => {
            if let Some(cell) = heap.cells.get(*key) {
                match &cell.value {
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
                    }
                    HeapData::String(s) => s.clone(),
                }
            } else {
                "<dead>".to_string()
            }
        }
    }
}
