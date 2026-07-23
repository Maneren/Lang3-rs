use crate::primitive::{Primitive, compare_primitives};
use crate::stack_value::StackValue;
use crate::function::Function;
use crate::heap::Heap;
use std::cmp::Ordering;

#[derive(Debug, Clone)]
pub enum HeapData {
    Nil,
    Primitive(Primitive),
    Function(Function),
    Vector(Vec<StackValue>),
    String(String),
}

impl HeapData {
    pub fn is_nil(&self) -> bool {
        matches!(self, HeapData::Nil)
    }

    pub fn is_primitive(&self) -> bool {
        matches!(self, HeapData::Primitive(_))
    }

    pub fn is_function(&self) -> bool {
        matches!(self, HeapData::Function(_))
    }

    pub fn is_vector(&self) -> bool {
        matches!(self, HeapData::Vector(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, HeapData::String(_))
    }

    pub fn as_primitive(&self) -> Option<Primitive> {
        if let HeapData::Primitive(p) = self { Some(*p) } else { None }
    }

    pub fn as_vector(&self) -> Option<&Vec<StackValue>> {
        if let HeapData::Vector(v) = self { Some(v) } else { None }
    }

    pub fn as_mut_vector(&mut self) -> Option<&mut Vec<StackValue>> {
        if let HeapData::Vector(v) = self { Some(v) } else { None }
    }

    pub fn as_string(&self) -> Option<&str> {
        if let HeapData::String(s) = self { Some(s.as_str()) } else { None }
    }

    pub fn as_mut_string(&mut self) -> Option<&mut String> {
        if let HeapData::String(s) = self { Some(s) } else { None }
    }

    pub fn type_name(&self, _heap: &Heap) -> &'static str {
        match self {
            HeapData::Nil => "nil",
            HeapData::Primitive(p) => p.type_name(),
            HeapData::Function(_) => "function",
            HeapData::Vector(_) => "vector",
            HeapData::String(_) => "string",
        }
    }

    pub fn is_truthy(&self, _heap: &Heap) -> bool {
        match self {
            HeapData::Nil => false,
            HeapData::Primitive(p) => p.is_truthy(),
            HeapData::Function(_) => true,
            HeapData::Vector(v) => !v.is_empty(),
            HeapData::String(s) => !s.is_empty(),
        }
    }

    pub fn fmt_with_heap(&self, heap: &Heap) -> String {
        match self {
            HeapData::Nil => "nil".to_string(),
            HeapData::Primitive(p) => format!("{}", p),
            HeapData::Function(f) => match f {
                crate::function::Function::Builtin(b) => format!("<builtin {}>", b.name),
                crate::function::Function::Bytecode(bc) => format!("<fn {}>", bc.name),
            },
            HeapData::Vector(v) => {
                let elems: Vec<String> = v.iter().map(|sv| format_stack_value(sv, heap)).collect();
                format!("[{}]", elems.join(", "))
            }
            HeapData::String(s) => format!("\"{}\"", s),
        }
    }
}

// Display free function using heap
pub fn format_stack_value(sv: &StackValue, heap: &Heap) -> String {
    match sv {
        StackValue::Nil => "nil".to_string(),
        StackValue::Primitive(p) => format!("{}", p),
        StackValue::Heap(key) => {
            if let Some(cell) = heap.cells.get(*key) {
                cell.value.fmt_with_heap(heap)
            } else {
                "<dead>".to_string()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Arithmetic operations on StackValues
// ---------------------------------------------------------------------------

fn extract_primitive(sv: &StackValue, heap: &Heap) -> Option<Primitive> {
    match sv {
        StackValue::Primitive(p) => Some(*p),
        StackValue::Heap(key) => {
            heap.cells.get(*key)?.value.as_primitive()
        }
        StackValue::Nil => None,
    }
}

pub fn add(a: &StackValue, b: &StackValue, heap: &mut Heap) -> Result<StackValue, crate::error::RuntimeError> {
    // String + String
    if let (Some(sa), Some(sb)) = (as_heap_string(a, heap), as_heap_string(b, heap)) {
        let result = format!("{}{}", sa, sb);
        return Ok(heap.alloc_string(result));
    }
    // Vector + Vector
    if let (Some(va), Some(vb)) = (as_heap_vector(a, heap), as_heap_vector(b, heap)) {
        let mut result = va.clone();
        result.extend(vb.iter().cloned());
        return Ok(heap.alloc_vector(result));
    }
    // Numeric
    if let (Some(pa), Some(pb)) = (extract_primitive(a, heap), extract_primitive(b, heap)) {
        match pa + pb {
            Ok(p) => return Ok(StackValue::Primitive(p)),
            Err(e) => return Err(crate::error::RuntimeError::type_error(e)),
        }
    }
    Err(crate::error::RuntimeError::type_error("unsupported operand types for +"))
}

pub fn sub(a: &StackValue, b: &StackValue, heap: &Heap) -> Result<StackValue, crate::error::RuntimeError> {
    if let (Some(pa), Some(pb)) = (extract_primitive(a, heap), extract_primitive(b, heap)) {
        match pa - pb {
            Ok(p) => return Ok(StackValue::Primitive(p)),
            Err(e) => return Err(crate::error::RuntimeError::type_error(e)),
        }
    }
    Err(crate::error::RuntimeError::type_error("unsupported operand types for -"))
}

pub fn mul(a: &StackValue, b: &StackValue, heap: &mut Heap) -> Result<StackValue, crate::error::RuntimeError> {
    // String * Integer (repeat)
    if let (Some(s), Some(Primitive::Integer(n))) = (as_heap_string(a, heap), extract_primitive(b, heap)) {
        if n <= 0 {
            return Ok(heap.alloc_string(String::new()));
        }
        let result = s.repeat(n.max(0) as usize);
        return Ok(heap.alloc_string(result));
    }
    // Integer * String (repeat)
    if let (Some(Primitive::Integer(n)), Some(s)) = (extract_primitive(a, heap), as_heap_string(b, heap)) {
        if n <= 0 {
            return Ok(heap.alloc_string(String::new()));
        }
        let result = s.repeat(n.max(0) as usize);
        return Ok(heap.alloc_string(result));
    }
    // Vector * Integer (repeat)
    if let (Some(v), Some(Primitive::Integer(n))) = (as_heap_vector(a, heap), extract_primitive(b, heap)) {
        if n <= 0 {
            return Ok(heap.alloc_vector(Vec::new()));
        }
        let mut result = Vec::new();
        for _ in 0..n.max(0) {
            result.extend(v.iter().cloned());
        }
        return Ok(heap.alloc_vector(result));
    }
    if let (Some(pa), Some(pb)) = (extract_primitive(a, heap), extract_primitive(b, heap)) {
        match pa * pb {
            Ok(p) => return Ok(StackValue::Primitive(p)),
            Err(e) => return Err(crate::error::RuntimeError::type_error(e)),
        }
    }
    Err(crate::error::RuntimeError::type_error("unsupported operand types for *"))
}

pub fn div(a: &StackValue, b: &StackValue, heap: &Heap) -> Result<StackValue, crate::error::RuntimeError> {
    if let (Some(pa), Some(pb)) = (extract_primitive(a, heap), extract_primitive(b, heap)) {
        match pa / pb {
            Ok(p) => return Ok(StackValue::Primitive(p)),
            Err(e) => return Err(crate::error::RuntimeError::type_error(e)),
        }
    }
    Err(crate::error::RuntimeError::type_error("unsupported operand types for /"))
}

pub fn modulo(a: &StackValue, b: &StackValue, heap: &Heap) -> Result<StackValue, crate::error::RuntimeError> {
    if let (Some(pa), Some(pb)) = (extract_primitive(a, heap), extract_primitive(b, heap)) {
        match pa % pb {
            Ok(p) => return Ok(StackValue::Primitive(p)),
            Err(e) => return Err(crate::error::RuntimeError::type_error(e)),
        }
    }
    Err(crate::error::RuntimeError::type_error("unsupported operand types for %"))
}

pub fn pow(a: &StackValue, b: &StackValue, heap: &Heap) -> Result<StackValue, crate::error::RuntimeError> {
    if let (Some(pa), Some(pb)) = (extract_primitive(a, heap), extract_primitive(b, heap)) {
        match (pa, pb) {
            (Primitive::Integer(a), Primitive::Integer(b)) => {
                let result = a.wrapping_pow(b as u32);
                Ok(StackValue::Primitive(Primitive::Integer(result)))
            }
            (Primitive::Double(a), Primitive::Double(b)) => {
                Ok(StackValue::Primitive(Primitive::Double(a.powf(b))))
            }
            (Primitive::Integer(a), Primitive::Double(b)) => {
                Ok(StackValue::Primitive(Primitive::Double((a as f64).powf(b))))
            }
            (Primitive::Double(a), Primitive::Integer(b)) => {
                Ok(StackValue::Primitive(Primitive::Double(a.powi(b as i32))))
            }
            _ => Err(crate::error::RuntimeError::type_error("unsupported operand types for ^")),
        }
    } else {
        Err(crate::error::RuntimeError::type_error("unsupported operand types for ^"))
    }
}

pub fn compare(a: &StackValue, b: &StackValue, _heap: &Heap) -> Option<Ordering> {
    match (a, b) {
        (StackValue::Primitive(pa), StackValue::Primitive(pb)) => compare_primitives(*pa, *pb),
        (StackValue::Nil, StackValue::Nil) => Some(Ordering::Equal),
        _ => None,
    }
}

pub fn negative(a: &StackValue, heap: &Heap) -> Result<StackValue, crate::error::RuntimeError> {
    if let Some(p) = extract_primitive(a, heap) {
        Ok(StackValue::Primitive(-p))
    } else {
        Err(crate::error::RuntimeError::type_error("unsupported operand types for unary -"))
    }
}

pub fn not_op(a: &StackValue, heap: &Heap) -> StackValue {
    StackValue::Primitive(Primitive::Bool(!a.is_truthy(heap)))
}

pub fn index(container: &StackValue, index: &StackValue, heap: &mut Heap) -> Result<StackValue, crate::error::RuntimeError> {
    let idx = index_value(index, heap)?;

    if idx < 0 {
        return Err(crate::error::RuntimeError::value("index out of bounds"));
    }
    if let Some(s) = as_heap_string(container, heap) {
        let chars: Vec<char> = s.chars().collect();
        let i = idx as usize;
        if i >= chars.len() {
            return Err(crate::error::RuntimeError::value("index out of bounds"));
        }
        Ok(heap.alloc_string(chars[i].to_string()))
    } else if let Some(v) = as_heap_vector(container, heap) {
        let i = idx as usize;
        if i >= v.len() {
            return Err(crate::error::RuntimeError::value("index out of bounds"));
        }
        Ok(v[i].clone())
    } else {
        let type_name = if let Some(p) = extract_primitive(container, heap) {
            match p {
                Primitive::Integer(_) => "int",
                Primitive::Double(_) => "float",
                Primitive::Bool(_) => "bool",
            }
        } else {
            "non-container"
        };
        Err(crate::error::RuntimeError::type_error(format!("cannot index a {} value", type_name)))
    }
}

pub fn index_mut<'a>(container: &'a mut StackValue, index: &StackValue, heap: &'a mut Heap) -> Result<&'a mut StackValue, crate::error::RuntimeError> {
    let idx = index_value(index, heap)?;

    // We need to get the vector out of the heap
    let key = if let StackValue::Heap(k) = container { *k } else {
        return Err(crate::error::RuntimeError::type_error("cannot index non-container type"));
    };

    // Drop the borrow on container, then use key to access heap
    let cell = heap.cells.get_mut(key).ok_or_else(|| crate::error::RuntimeError::value("invalid heap reference"))?;
    let v = cell.value.as_mut_vector().ok_or_else(|| crate::error::RuntimeError::type_error("cannot index non-vector type"))?;
    if idx < 0 {
        return Err(crate::error::RuntimeError::value("index out of bounds"));
    }
    let i = idx as usize;
    if i >= v.len() {
        return Err(crate::error::RuntimeError::value("index out of bounds"));
    }
    Ok(&mut v[i])
}

fn index_value(sv: &StackValue, heap: &Heap) -> Result<i64, crate::error::RuntimeError> {
    if let Some(Primitive::Integer(i)) = extract_primitive(sv, heap) {
        return Ok(i);
    }
    Err(crate::error::RuntimeError::type_error("index to a container must be an integer"))
}

fn as_heap_string<'a>(sv: &'a StackValue, heap: &'a Heap) -> Option<&'a str> {
    if let StackValue::Heap(key) = sv {
        heap.cells.get(*key).and_then(|c| c.value.as_string())
    } else {
        None
    }
}

fn as_heap_vector<'a>(sv: &'a StackValue, heap: &'a Heap) -> Option<&'a Vec<StackValue>> {
    if let StackValue::Heap(key) = sv {
        heap.cells.get(*key).and_then(|c| c.value.as_vector())
    } else {
        None
    }
}

pub fn to_owned(sv: &StackValue, heap: &Heap) -> HeapData {
    match sv {
        StackValue::Nil => HeapData::Nil,
        StackValue::Primitive(p) => HeapData::Primitive(*p),
        StackValue::Heap(key) => {
            if let Some(cell) = heap.cells.get(*key) {
                cell.value.clone()
            } else {
                HeapData::Nil
            }
        }
    }
}
