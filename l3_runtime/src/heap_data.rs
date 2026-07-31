use crate::function::Function;
use crate::heap::Heap;
use crate::primitive::{Primitive, compare_primitives};
use crate::stack_value::StackValue;
use slotmap::DefaultKey;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

impl fmt::Display for HeapData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapData::Nil => write!(f, "nil"),
            HeapData::Primitive(p) => write!(f, "{p}"),
            HeapData::Function(fun) => match fun {
                Function::Builtin(b) => write!(f, "function <{}>", b.name),
                Function::Bytecode(bc) => write!(f, "function <{}>", bc.name),
            },
            HeapData::Vector(v) => {
                write!(f, "[")?;
                for (i, sv) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{sv}")?;
                }
                write!(f, "]")
            }
            HeapData::String(s) => write!(f, "\"{s}\""),
        }
    }
}

#[derive(Debug, Clone)]
pub enum HeapData {
    Nil,
    Primitive(Primitive),
    Function(Function),
    Vector(Vec<StackValue>),
    String(String),
}

impl PartialEq for HeapData {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (HeapData::Nil, HeapData::Nil) => true,
            (HeapData::Primitive(a), HeapData::Primitive(b)) => a == b,
            (HeapData::String(a), HeapData::String(b)) => a == b,
            _ => false,
        }
    }
}

impl Hash for HeapData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            HeapData::Nil => 0u8.hash(state),
            HeapData::Primitive(p) => {
                1u8.hash(state);
                p.hash(state);
            }
            HeapData::String(s) => {
                2u8.hash(state);
                s.hash(state);
            }
            HeapData::Function(_) => {
                3u8.hash(state);
                // Functions are unique — use a random-ish constant
            }
            HeapData::Vector(v) => {
                v.len().hash(state);
                4u8.hash(state);
            }
        }
    }
}

impl HeapData {
    #[must_use]
    pub fn is_nil(&self) -> bool {
        matches!(self, HeapData::Nil)
    }

    #[must_use]
    pub fn is_primitive(&self) -> bool {
        matches!(self, HeapData::Primitive(_))
    }

    #[must_use]
    pub fn is_function(&self) -> bool {
        matches!(self, HeapData::Function(_))
    }

    #[must_use]
    pub fn is_vector(&self) -> bool {
        matches!(self, HeapData::Vector(_))
    }

    #[must_use]
    pub fn is_string(&self) -> bool {
        matches!(self, HeapData::String(_))
    }

    #[must_use]
    pub fn as_primitive(&self) -> Option<Primitive> {
        if let HeapData::Primitive(p) = self {
            Some(*p)
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_vector(&self) -> Option<&Vec<StackValue>> {
        if let HeapData::Vector(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_mut_vector(&mut self) -> Option<&mut Vec<StackValue>> {
        if let HeapData::Vector(v) = self {
            Some(v)
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_string(&self) -> Option<&str> {
        if let HeapData::String(s) = self {
            Some(s.as_str())
        } else {
            None
        }
    }

    pub fn as_mut_string(&mut self) -> Option<&mut String> {
        if let HeapData::String(s) = self {
            Some(s)
        } else {
            None
        }
    }

    #[must_use]
    pub fn type_name(&self, _heap: &Heap) -> &'static str {
        match self {
            HeapData::Nil => "nil",
            HeapData::Primitive(p) => p.type_name(),
            HeapData::Function(_) => "function",
            HeapData::Vector(_) => "vector",
            HeapData::String(_) => "string",
        }
    }

    #[must_use]
    pub fn is_truthy(&self, _heap: &Heap) -> bool {
        match self {
            HeapData::Nil => false,
            HeapData::Primitive(p) => p.is_truthy(),
            HeapData::Function(_) => true,
            HeapData::Vector(v) => !v.is_empty(),
            HeapData::String(s) => !s.is_empty(),
        }
    }

    #[must_use]
    pub fn fmt_with_heap(&self, heap: &Heap) -> String {
        match self {
            HeapData::Nil => "nil".to_string(),
            HeapData::Primitive(p) => format!("{p}"),
            HeapData::Function(f) => match f {
                crate::function::Function::Builtin(b) => format!("<builtin {}>", b.name),
                crate::function::Function::Bytecode(bc) => format!("<fn {}>", bc.name),
            },
            HeapData::Vector(v) => {
                let elems: Vec<String> = v.iter().map(|sv| format_stack_value(sv, heap)).collect();
                format!("[{}]", elems.join(", "))
            }
            HeapData::String(s) => format!("\"{s}\""),
        }
    }
}

// Display free function using heap
#[must_use]
pub fn format_stack_value(sv: &StackValue, heap: &Heap) -> String {
    match sv {
        StackValue::Nil => "nil".to_string(),
        StackValue::Primitive(p) => format!("{p}"),
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

/// An operand resolved to the heap-data kind it can take part in.
enum Resolved<'a> {
    Num(Primitive),
    Str(&'a str),
    Vec(&'a Vec<StackValue>),
    Other,
}

/// Resolve a StackValue to its effective kind with a single heap lookup.
fn resolve<'a>(sv: &'a StackValue, heap: &'a Heap) -> Resolved<'a> {
    match sv {
        StackValue::Primitive(p) => Resolved::Num(*p),
        StackValue::Heap(key) => match heap.cells.get(*key).map(|c| &c.value) {
            Some(HeapData::Primitive(p)) => Resolved::Num(*p),
            Some(HeapData::String(s)) => Resolved::Str(s),
            Some(HeapData::Vector(v)) => Resolved::Vec(v),
            _ => Resolved::Other,
        },
        StackValue::Nil => Resolved::Other,
    }
}

fn numeric_result(
    r: Result<Primitive, &'static str>,
) -> Result<StackValue, crate::error::RuntimeError> {
    r.map(StackValue::Primitive)
        .map_err(crate::error::RuntimeError::type_error)
}

pub fn add(
    a: &StackValue,
    b: &StackValue,
    heap: &mut Heap,
) -> Result<StackValue, crate::error::RuntimeError> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa + pb),
        (Resolved::Str(sa), Resolved::Str(sb)) => Ok(heap.alloc_string(format!("{sa}{sb}"))),
        (Resolved::Vec(va), Resolved::Vec(vb)) => {
            let mut result = va.clone();
            result.extend(vb.iter().cloned());
            Ok(heap.alloc_vector(result))
        }
        _ => Err(crate::error::RuntimeError::type_error(
            "unsupported operand types for +",
        )),
    }
}

pub fn sub(
    a: &StackValue,
    b: &StackValue,
    heap: &Heap,
) -> Result<StackValue, crate::error::RuntimeError> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa - pb),
        _ => Err(crate::error::RuntimeError::type_error(
            "unsupported operand types for -",
        )),
    }
}

fn repeat_str(s: &str, n: i64) -> String {
    if n <= 0 {
        String::new()
    } else {
        s.repeat(n as usize)
    }
}

fn repeat_vec(v: &[StackValue], n: i64) -> Vec<StackValue> {
    if n <= 0 {
        Vec::new()
    } else {
        let mut result = Vec::new();
        for _ in 0..n {
            result.extend(v.iter().cloned());
        }
        result
    }
}

pub fn mul(
    a: &StackValue,
    b: &StackValue,
    heap: &mut Heap,
) -> Result<StackValue, crate::error::RuntimeError> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa * pb),
        (Resolved::Num(Primitive::Integer(n)), Resolved::Str(s))
        | (Resolved::Str(s), Resolved::Num(Primitive::Integer(n))) => {
            Ok(heap.alloc_string(repeat_str(s, n)))
        }
        (Resolved::Vec(v), Resolved::Num(Primitive::Integer(n))) => {
            Ok(heap.alloc_vector(repeat_vec(v, n)))
        }
        _ => Err(crate::error::RuntimeError::type_error(
            "unsupported operand types for *",
        )),
    }
}

pub fn div(
    a: &StackValue,
    b: &StackValue,
    heap: &Heap,
) -> Result<StackValue, crate::error::RuntimeError> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa / pb),
        _ => Err(crate::error::RuntimeError::type_error(
            "unsupported operand types for /",
        )),
    }
}

pub fn modulo(
    a: &StackValue,
    b: &StackValue,
    heap: &Heap,
) -> Result<StackValue, crate::error::RuntimeError> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa % pb),
        _ => Err(crate::error::RuntimeError::type_error(
            "unsupported operand types for %",
        )),
    }
}

pub fn pow(
    a: &StackValue,
    b: &StackValue,
    heap: &Heap,
) -> Result<StackValue, crate::error::RuntimeError> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => {
            let result = match (pa, pb) {
                (Primitive::Integer(a), Primitive::Integer(b)) => {
                    Primitive::Integer(a.wrapping_pow(b as u32))
                }
                (Primitive::Double(a), Primitive::Double(b)) => Primitive::Double(a.powf(b)),
                (Primitive::Integer(a), Primitive::Double(b)) => {
                    Primitive::Double((a as f64).powf(b))
                }
                (Primitive::Double(a), Primitive::Integer(b)) => {
                    Primitive::Double(a.powi(b as i32))
                }
                _ => {
                    return Err(crate::error::RuntimeError::type_error(
                        "unsupported operand types for ^",
                    ));
                }
            };
            Ok(StackValue::Primitive(result))
        }
        _ => Err(crate::error::RuntimeError::type_error(
            "unsupported operand types for ^",
        )),
    }
}

#[must_use]
pub fn compare(a: &StackValue, b: &StackValue, _heap: &Heap) -> Option<Ordering> {
    match (a, b) {
        (StackValue::Primitive(pa), StackValue::Primitive(pb)) => compare_primitives(*pa, *pb),
        (StackValue::Nil, StackValue::Nil) => Some(Ordering::Equal),
        _ => None,
    }
}

pub fn negative(a: &StackValue, heap: &Heap) -> Result<StackValue, crate::error::RuntimeError> {
    match resolve(a, heap) {
        Resolved::Num(p) => Ok(StackValue::Primitive(-p)),
        _ => Err(crate::error::RuntimeError::type_error(
            "unsupported operand types for unary -",
        )),
    }
}

#[must_use]
pub fn not_op(a: &StackValue, heap: &Heap) -> StackValue {
    StackValue::Primitive(Primitive::Bool(!a.is_truthy(heap)))
}

fn integer_index(sv: &StackValue, heap: &Heap) -> Result<i64, crate::error::RuntimeError> {
    match resolve(sv, heap) {
        Resolved::Num(Primitive::Integer(i)) => Ok(i),
        _ => Err(crate::error::RuntimeError::type_error(
            "index to a container must be an integer",
        )),
    }
}

fn heap_key(container: &StackValue) -> Result<DefaultKey, crate::error::RuntimeError> {
    match container {
        StackValue::Heap(key) => Ok(*key),
        _ => Err(crate::error::RuntimeError::type_error(
            "cannot index non-container type",
        )),
    }
}

pub fn index(
    container: &StackValue,
    index: &StackValue,
    heap: &mut Heap,
) -> Result<StackValue, crate::error::RuntimeError> {
    let idx = integer_index(index, heap)?;
    if idx < 0 {
        return Err(crate::error::RuntimeError::value("index out of bounds"));
    }
    let i = idx as usize;
    match resolve(container, heap) {
        Resolved::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            if i >= chars.len() {
                return Err(crate::error::RuntimeError::value("index out of bounds"));
            }
            Ok(heap.alloc_string(chars[i].to_string()))
        }
        Resolved::Vec(v) => {
            if i >= v.len() {
                return Err(crate::error::RuntimeError::value("index out of bounds"));
            }
            Ok(v[i].clone())
        }
        Resolved::Num(p) => {
            let type_name = match p {
                Primitive::Integer(_) => "int",
                Primitive::Double(_) => "float",
                Primitive::Bool(_) => "bool",
            };
            Err(crate::error::RuntimeError::type_error(format!(
                "cannot index a {type_name} value"
            )))
        }
        Resolved::Other => Err(crate::error::RuntimeError::type_error(
            "cannot index a non-container value",
        )),
    }
}

pub fn index_mut<'a>(
    container: &'a mut StackValue,
    index: &StackValue,
    heap: &'a mut Heap,
) -> Result<&'a mut StackValue, crate::error::RuntimeError> {
    let idx = integer_index(index, heap)?;
    let key = heap_key(container)?;

    let cell = heap
        .cells
        .get_mut(key)
        .ok_or_else(|| crate::error::RuntimeError::value("invalid heap reference"))?;
    let v = cell
        .value
        .as_mut_vector()
        .ok_or_else(|| crate::error::RuntimeError::type_error("cannot index non-vector type"))?;
    if idx < 0 {
        return Err(crate::error::RuntimeError::value("index out of bounds"));
    }
    let i = idx as usize;
    if i >= v.len() {
        return Err(crate::error::RuntimeError::value("index out of bounds"));
    }
    Ok(&mut v[i])
}

#[must_use]
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
