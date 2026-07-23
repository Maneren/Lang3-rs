use std::rc::Rc;
use l3_runtime::*;

type Builtin = Rc<dyn Fn(Vec<StackValue>, &mut Heap) -> Result<StackValue, RuntimeError>>;

pub fn builtins() -> Vec<(&'static str, Builtin)> {
    vec![
        ("print", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            let mut line = String::new();
            for (i, arg) in args.iter().enumerate() {
                if i > 0 { line.push(' '); }
                line.push_str(&format_stack_value(arg, heap));
            }
            heap.output_lines.push(line);
            Ok(StackValue::Nil)
        })),
        ("println", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            let mut line = String::new();
            for (i, arg) in args.iter().enumerate() {
                if i > 0 { line.push(' '); }
                line.push_str(&format_stack_value(arg, heap));
            }
            heap.output_lines.push(line);
            Ok(StackValue::Nil)
        })),
        ("assert", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if args.len() < 1 {
                return Err(RuntimeError::type_error("assert requires at least 1 argument"));
            }
            if !args[0].is_truthy(heap) {
                let msg = if args.len() > 1 {
                    format_stack_value(&args[1], heap)
                } else {
                    "assertion failed".to_string()
                };
                heap.output_lines.push(format!("AssertionError: {}", msg));
                return Err(RuntimeError::value(format!("assertion failed: {}", msg)));
            }
            Ok(StackValue::Nil)
        })),
        ("error", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            let msg = if !args.is_empty() {
                format_stack_value(&args[0], heap)
            } else {
                "error".to_string()
            };
            heap.output_lines.push(format!("Error: {}", msg));
            Err(RuntimeError::value(format!("error: {}", msg)))
        })),
        ("int", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if args.is_empty() {
                return Ok(StackValue::Primitive(Primitive::Integer(0)));
            }
            let sv = &args[0];
            match sv {
                StackValue::Primitive(Primitive::Integer(i)) => Ok(StackValue::Primitive(Primitive::Integer(*i))),
                StackValue::Primitive(Primitive::Double(f)) => Ok(StackValue::Primitive(Primitive::Integer(*f as i64))),
                StackValue::Primitive(Primitive::Bool(b)) => Ok(StackValue::Primitive(Primitive::Integer(if *b { 1 } else { 0 }))),
                StackValue::Heap(key) => {
                    if let Some(cell) = heap.cells.get(*key) {
                        if let Some(s) = cell.value.as_string() {
                            if let Ok(n) = s.parse::<i64>() {
                                return Ok(StackValue::Primitive(Primitive::Integer(n)));
                            }
                        }
                    }
                    Ok(StackValue::Primitive(Primitive::Integer(0)))
                }
                StackValue::Nil => Ok(StackValue::Primitive(Primitive::Integer(0))),
            }
        })),
        ("str", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if args.is_empty() {
                return Ok(heap.alloc_string(String::new()));
            }
            Ok(heap.alloc_string(format_stack_value(&args[0], heap)))
        })),
        ("len", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if args.is_empty() {
                return Ok(StackValue::Primitive(Primitive::Integer(0)));
            }
            let sv = &args[0];
            match sv {
                StackValue::Heap(key) => {
                    if let Some(cell) = heap.cells.get(*key) {
                        match &cell.value {
                            HeapData::String(s) => Ok(StackValue::Primitive(Primitive::Integer(s.len() as i64))),
                            HeapData::Vector(v) => Ok(StackValue::Primitive(Primitive::Integer(v.len() as i64))),
                            _ => Ok(StackValue::Primitive(Primitive::Integer(0))),
                        }
                    } else {
                        Ok(StackValue::Primitive(Primitive::Integer(0)))
                    }
                }
                _ => Ok(StackValue::Primitive(Primitive::Integer(0))),
            }
        })),
        ("head", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if args.is_empty() {
                return Err(RuntimeError::type_error("head requires an argument"));
            }
            let sv = &args[0];
            match sv {
                StackValue::Heap(key) => {
                    if let Some(cell) = heap.cells.get(*key) {
                        match &cell.value {
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
                                let c = s.chars().next().unwrap();
                                Ok(heap.alloc_string(c.to_string()))
                            }
                            _ => Err(RuntimeError::type_error("head requires a vector or string")),
                        }
                    } else {
                        Err(RuntimeError::type_error("invalid heap reference"))
                    }
                }
                _ => Err(RuntimeError::type_error("head requires a vector or string")),
            }
        })),
        ("tail", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if args.is_empty() {
                return Err(RuntimeError::type_error("tail requires an argument"));
            }
            let sv = &args[0];
            match sv {
                StackValue::Heap(key) => {
                    if let Some(cell) = heap.cells.get(*key) {
                        match &cell.value {
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
                                let tail: String = s.chars().skip(1).collect();
                                Ok(heap.alloc_string(tail))
                            }
                            _ => Err(RuntimeError::type_error("tail requires a vector or string")),
                        }
                    } else {
                        Err(RuntimeError::type_error("invalid heap reference"))
                    }
                }
                _ => Err(RuntimeError::type_error("tail requires a vector or string")),
            }
        })),
        ("drop", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if args.len() < 2 {
                return Err(RuntimeError::type_error("drop requires 2 arguments"));
            }
            let n = match args[1].as_primitive() {
                Some(Primitive::Integer(i)) => i as usize,
                _ => return Err(RuntimeError::type_error("drop requires integer count")),
            };
            match &args[0] {
                StackValue::Heap(key) => {
                    if let Some(cell) = heap.cells.get(*key) {
                        match &cell.value {
                            HeapData::Vector(v) => {
                                if n >= v.len() {
                                    return Ok(heap.alloc_vector(Vec::new()));
                                }
                                Ok(heap.alloc_vector(v[n..].to_vec()))
                            }
                            HeapData::String(s) => {
                                let tail: String = s.chars().skip(n).collect();
                                Ok(heap.alloc_string(tail))
                            }
                            _ => Err(RuntimeError::type_error("drop requires a vector or string")),
                        }
                    } else {
                        Err(RuntimeError::type_error("invalid heap reference"))
                    }
                }
                _ => Err(RuntimeError::type_error("drop requires a vector or string")),
            }
        })),
        ("take", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if args.len() < 2 {
                return Err(RuntimeError::type_error("take requires 2 arguments"));
            }
            let n = match args[1].as_primitive() {
                Some(Primitive::Integer(i)) => i as usize,
                _ => return Err(RuntimeError::type_error("take requires integer count")),
            };
            match &args[0] {
                StackValue::Heap(key) => {
                    if let Some(cell) = heap.cells.get(*key) {
                        match &cell.value {
                            HeapData::Vector(v) => {
                                let end = n.min(v.len());
                                Ok(heap.alloc_vector(v[..end].to_vec()))
                            }
                            HeapData::String(s) => {
                                let head: String = s.chars().take(n).collect();
                                Ok(heap.alloc_string(head))
                            }
                            _ => Err(RuntimeError::type_error("take requires a vector or string")),
                        }
                    } else {
                        Err(RuntimeError::type_error("invalid heap reference"))
                    }
                }
                _ => Err(RuntimeError::type_error("take requires a vector or string")),
            }
        })),
        ("range", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            let start = if args.len() > 0 {
                match args[0].as_primitive() {
                    Some(Primitive::Integer(i)) => i,
                    _ => return Err(RuntimeError::type_error("range requires integer arguments")),
                }
            } else {
                0
            };
            let end = if args.len() > 1 {
                match args[1].as_primitive() {
                    Some(Primitive::Integer(i)) => i,
                    _ => return Err(RuntimeError::type_error("range requires integer arguments")),
                }
            } else {
                start
            };
            let step = if args.len() > 2 {
                match args[2].as_primitive() {
                    Some(Primitive::Integer(i)) if i != 0 => i,
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
        })),
        ("id", Rc::new(|args: Vec<StackValue>, _heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            Ok(args.into_iter().next().unwrap_or(StackValue::Nil))
        })),
        ("map", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if args.len() < 2 {
                return Err(RuntimeError::type_error("map requires a function and a vector"));
            }
            let func_sv = &args[0];
            let func_data = match func_sv {
                StackValue::Heap(key) => {
                    if let Some(cell) = heap.cells.get(*key) {
                        cell.value.clone()
                    } else {
                        return Err(RuntimeError::type_error("invalid function reference"));
                    }
                }
                _ => return Err(RuntimeError::type_error("map requires a function as first argument")),
            };
            let vec_sv = &args[1];
            let vec = match vec_sv {
                StackValue::Heap(key) => {
                    if let Some(cell) = heap.cells.get(*key) {
                        if let HeapData::Vector(v) = &cell.value {
                            v.clone()
                        } else {
                            return Err(RuntimeError::type_error("map requires a vector as second argument"));
                        }
                    } else {
                        return Err(RuntimeError::type_error("invalid heap reference"));
                    }
                }
                _ => return Err(RuntimeError::type_error("map requires a vector as second argument")),
            };
            match func_data {
                HeapData::Function(Function::Builtin(b)) => {
                    let mut result = Vec::new();
                    for elem in &vec {
                        let r = b.invoke(vec![elem.clone()], heap)?;
                        result.push(r);
                    }
                    Ok(heap.alloc_vector(result))
                }
                _ => Err(RuntimeError::type_error("map currently only supports builtin functions")),
            }
        })),
        ("count", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if args.len() < 2 {
                return Err(RuntimeError::type_error("count requires a predicate and a vector"));
            }
            let func_sv = &args[0];
            let func_data = match func_sv {
                StackValue::Heap(key) => {
                    if let Some(cell) = heap.cells.get(*key) {
                        cell.value.clone()
                    } else {
                        return Err(RuntimeError::type_error("invalid function reference"));
                    }
                }
                _ => return Err(RuntimeError::type_error("count requires a function as first argument")),
            };
            let vec_sv = &args[1];
            let vec = match vec_sv {
                StackValue::Heap(key) => {
                    if let Some(cell) = heap.cells.get(*key) {
                        if let HeapData::Vector(v) = &cell.value {
                            v.clone()
                        } else {
                            return Err(RuntimeError::type_error("count requires a vector as second argument"));
                        }
                    } else {
                        return Err(RuntimeError::type_error("invalid heap reference"));
                    }
                }
                _ => return Err(RuntimeError::type_error("count requires a vector as second argument")),
            };
            match func_data {
                HeapData::Function(Function::Builtin(b)) => {
                    let mut total = 0i64;
                    for elem in &vec {
                        let r = b.invoke(vec![elem.clone()], heap)?;
                        if r.is_truthy(heap) {
                            total += 1;
                        }
                    }
                    Ok(StackValue::Primitive(Primitive::Integer(total)))
                }
                _ => Err(RuntimeError::type_error("count currently only supports builtin functions")),
            }
        })),
        ("random", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            heap.rng_state = heap.rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let val = heap.rng_state;
            let limit = if !args.is_empty() {
                match args[0].as_primitive() {
                    Some(Primitive::Integer(i)) if i > 0 => i as u64,
                    _ => return Err(RuntimeError::type_error("random requires a positive integer argument")),
                }
            } else {
                u64::MAX
            };
            Ok(StackValue::Primitive(Primitive::Integer((val % limit) as i64)))
        })),
        ("input", Rc::new(|_: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if let Some(line) = heap.input_queue.pop_front() {
                return Ok(heap.alloc_string(line));
            }
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).ok();
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') { line.pop(); }
            }
            Ok(heap.alloc_string(line))
        })),
        ("sleep", Rc::new(|args: Vec<StackValue>, _heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            let ms = if !args.is_empty() {
                match args[0].as_primitive() {
                    Some(Primitive::Integer(i)) if i >= 0 => i as u64,
                    _ => return Err(RuntimeError::type_error("sleep requires a non-negative integer argument")),
                }
            } else {
                0
            };
            std::thread::sleep(std::time::Duration::from_millis(ms));
            Ok(StackValue::Nil)
        })),
        ("sum", Rc::new(|args: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            if args.is_empty() {
                return Ok(StackValue::Primitive(Primitive::Integer(0)));
            }
            let mut total: f64 = 0.0;
            let mut is_int = true;
            if let StackValue::Heap(key) = &args[0] {
                if let Some(cell) = heap.cells.get(*key) {
                    if let HeapData::Vector(v) = &cell.value {
                        for sv in v {
                            match sv.as_primitive() {
                                Some(Primitive::Integer(i)) => total += i as f64,
                                Some(Primitive::Double(f)) => { total += f; is_int = false; }
                                _ => {}
                            }
                        }
                    }
                }
            }
            if is_int && total.fract() == 0.0 {
                Ok(StackValue::Primitive(Primitive::Integer(total as i64)))
            } else {
                Ok(StackValue::Primitive(Primitive::Double(total)))
            }
        })),
        ("__trigger_gc", Rc::new(|_: Vec<StackValue>, heap: &mut Heap| -> Result<StackValue, RuntimeError> {
            let erased = heap.sweep();
            Ok(heap.alloc_string(format!("GC swept {} cells", erased)))
        })),
    ]
}

pub fn format_stack_value(sv: &StackValue, heap: &Heap) -> String {
    match sv {
        StackValue::Nil => "nil".to_string(),
        StackValue::Primitive(p) => format!("{}", p),
        StackValue::Heap(key) => {
            if let Some(cell) = heap.cells.get(*key) {
                match &cell.value {
                    HeapData::Nil => "nil".to_string(),
                    HeapData::Primitive(p) => format!("{}", p),
                    HeapData::Function(f) => match f {
                        Function::Builtin(b) => format!("<builtin {}>", b.name),
                        Function::Bytecode(bc) => format!("<fn {}>", bc.name),
                    },
                    HeapData::Vector(v) => {
                        let elems: Vec<String> = v.iter().map(|sv| format_stack_value(sv, heap)).collect();
                        format!("[{}]", elems.join(", "))
                    }
                    HeapData::String(s) => format!("{}", s),
                }
            } else {
                "<dead>".to_string()
            }
        }
    }
}
