use std::{cmp::Ordering, collections::HashSet, fmt};

use slotmap::DefaultKey;

use crate::{
    error::{RuntimeError, RuntimeResult},
    function::Function,
    heap::Heap,
    primitive::{Primitive, compare_primitives},
    stack_value::StackValue,
};

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
            (Self::Nil, Self::Nil) => true,
            (Self::Primitive(a), Self::Primitive(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            _ => false,
        }
    }
}

impl HeapData {
    #[must_use]
    pub const fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }

    #[must_use]
    pub const fn is_primitive(&self) -> bool {
        matches!(self, Self::Primitive(_))
    }

    #[must_use]
    pub const fn is_function(&self) -> bool {
        matches!(self, Self::Function(_))
    }

    #[must_use]
    pub const fn is_vector(&self) -> bool {
        matches!(self, Self::Vector(_))
    }

    #[must_use]
    pub const fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    #[must_use]
    pub const fn as_primitive(&self) -> Option<Primitive> {
        if let Self::Primitive(p) = self {
            Some(*p)
        } else {
            None
        }
    }

    pub const fn as_mut_primitive(&mut self) -> Option<&mut Primitive> {
        if let Self::Primitive(p) = self {
            Some(p)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn as_vector(&self) -> Option<&Vec<StackValue>> {
        if let Self::Vector(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub const fn as_mut_vector(&mut self) -> Option<&mut Vec<StackValue>> {
        if let Self::Vector(v) = self {
            Some(v)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn as_string(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s.as_str())
        } else {
            None
        }
    }

    pub const fn as_mut_string(&mut self) -> Option<&mut String> {
        if let Self::String(s) = self {
            Some(s)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn as_function(&self) -> Option<&Function> {
        if let Self::Function(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub const fn as_mut_function(&mut self) -> Option<&mut Function> {
        if let Self::Function(v) = self {
            Some(v)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn type_name(&self, _heap: &Heap) -> &'static str {
        match self {
            Self::Nil => "nil",
            Self::Primitive(p) => p.type_name(),
            Self::Function(_) => "function",
            Self::Vector(_) => "vector",
            Self::String(_) => "string",
        }
    }

    #[must_use]
    pub fn is_truthy(&self, _heap: &Heap) -> bool {
        match self {
            Self::Nil => false,
            Self::Primitive(p) => p.is_truthy(),
            Self::Function(_) => true,
            Self::Vector(v) => !v.is_empty(),
            Self::String(s) => !s.is_empty(),
        }
    }

    #[must_use]
    pub fn fmt_with_heap(&self, heap: &Heap) -> String {
        match self {
            Self::Nil => "nil".to_string(),
            Self::Primitive(p) => format!("{p}"),
            Self::Function(f) => match f {
                Function::Builtin(b) => format!("<builtin {}>", b.name),
                Function::Bytecode(bc) => format!("<fn {}>", bc.name),
            },
            Self::Vector(v) => {
                let elems: Vec<String> = v.iter().map(|sv| format_stack_value(sv, heap)).collect();
                format!("[{}]", elems.join(", "))
            },
            Self::String(s) => format!("\"{s}\""),
        }
    }
}

// Display free function using heap
#[must_use]
pub fn format_stack_value(sv: &StackValue, heap: &Heap) -> String {
    match sv {
        StackValue::Nil => "nil".to_string(),
        StackValue::Primitive(p) => format!("{p}"),
        StackValue::Heap(key) => heap.cells.get(*key).map_or_else(
            || "<dead>".to_string(),
            |cell| cell.value.fmt_with_heap(heap),
        ),
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

/// Resolve a `StackValue` to its effective kind with a single heap lookup.
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

fn numeric_result(r: Result<Primitive, &'static str>) -> RuntimeResult<StackValue> {
    r.map(StackValue::Primitive)
        .map_err(RuntimeError::type_error)
}

pub fn add(a: &StackValue, b: &StackValue, heap: &mut Heap) -> RuntimeResult<StackValue> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa + pb),
        (Resolved::Str(sa), Resolved::Str(sb)) => Ok(heap.alloc_string(format!("{sa}{sb}"))),
        (Resolved::Vec(va), Resolved::Vec(vb)) => {
            let mut result = Vec::with_capacity(va.len() + vb.len());
            result.extend_from_slice(va);
            result.extend_from_slice(vb);
            Ok(heap.alloc_vector(result))
        },
        _ => Err(RuntimeError::type_error("unsupported operand types for +")),
    }
}

pub fn sub(a: &StackValue, b: &StackValue, heap: &mut Heap) -> RuntimeResult<StackValue> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa - pb),
        _ => Err(RuntimeError::type_error("unsupported operand types for -")),
    }
}

fn repeat_str(s: &str, n: i64) -> String {
    if n <= 0 {
        String::new()
    } else {
        s.repeat(usize::try_from(n).unwrap_or(0))
    }
}

fn repeat_vec(v: &[StackValue], n: i64) -> Vec<StackValue> {
    if n <= 0 {
        Vec::new()
    } else {
        let mut result = Vec::new();
        for _ in 0..n {
            result.extend(v.iter().copied());
        }
        result
    }
}

pub fn mul(a: &StackValue, b: &StackValue, heap: &mut Heap) -> RuntimeResult<StackValue> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa * pb),
        (Resolved::Num(Primitive::Integer(n)), Resolved::Str(s))
        | (Resolved::Str(s), Resolved::Num(Primitive::Integer(n))) => {
            Ok(heap.alloc_string(repeat_str(s, n)))
        },
        (Resolved::Vec(v), Resolved::Num(Primitive::Integer(n))) => {
            Ok(heap.alloc_vector(repeat_vec(v, n)))
        },
        _ => Err(RuntimeError::type_error("unsupported operand types for *")),
    }
}

pub fn div(a: &StackValue, b: &StackValue, heap: &mut Heap) -> RuntimeResult<StackValue> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa / pb),
        _ => Err(RuntimeError::type_error("unsupported operand types for /")),
    }
}

pub fn modulo(a: &StackValue, b: &StackValue, heap: &mut Heap) -> RuntimeResult<StackValue> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa % pb),
        _ => Err(RuntimeError::type_error("unsupported operand types for %")),
    }
}

pub fn pow(a: &StackValue, b: &StackValue, heap: &mut Heap) -> RuntimeResult<StackValue> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => {
            let result = match (pa, pb) {
                (Primitive::Integer(a), Primitive::Integer(b)) => {
                    if b < 0 {
                        // Promote negative integer exponents to double so that
                        // `2 ^ -1` is `0.5` (uniform with mixed-type promotion).
                        Primitive::Double((a as f64).powi(b as i32))
                    } else {
                        let Ok(exp) = u32::try_from(b) else {
                            return Err(RuntimeError::value(format!(
                                "exponent must be a non-negative 32-bit integer, got {b}"
                            )));
                        };
                        Primitive::Integer(a.wrapping_pow(exp))
                    }
                },
                (Primitive::Double(a), Primitive::Double(b)) => Primitive::Double(a.powf(b)),
                (Primitive::Integer(a), Primitive::Double(b)) => {
                    Primitive::Double((a as f64).powf(b))
                },
                (Primitive::Double(a), Primitive::Integer(b)) => {
                    Primitive::Double(a.powi(b as i32))
                },
                _ => {
                    return Err(RuntimeError::type_error("unsupported operand types for ^"));
                },
            };
            Ok(StackValue::Primitive(result))
        },
        _ => Err(RuntimeError::type_error("unsupported operand types for ^")),
    }
}

#[must_use]
pub fn compare(a: &StackValue, b: &StackValue, heap: &Heap) -> Option<Ordering> {
    compare_values(a, b, heap, &mut HashSet::new())
}

/// Content-based comparison: strings compare by value, vectors element-wise
/// (cycle-safe), everything else by reference identity (same heap key).
/// Ordering of container values is defined only through equality/inequality
/// outcomes; cross-type compares are `None`.
fn compare_values(
    a: &StackValue,
    b: &StackValue,
    heap: &Heap,
    seen: &mut HashSet<(DefaultKey, DefaultKey)>,
) -> Option<Ordering> {
    match (a, b) {
        (StackValue::Primitive(pa), StackValue::Primitive(pb)) => compare_primitives(*pa, *pb),
        (StackValue::Nil, StackValue::Nil) => Some(Ordering::Equal),
        (StackValue::Heap(ka), StackValue::Heap(kb)) => {
            if ka == kb {
                return Some(Ordering::Equal);
            }
            // Revisiting a pair means the structure is cyclic; assume equal.
            if !seen.insert((*ka, *kb)) {
                return Some(Ordering::Equal);
            }
            let result = match (
                heap.cells.get(*ka).map(|c| &c.value),
                heap.cells.get(*kb).map(|c| &c.value),
            ) {
                (Some(HeapData::String(sa)), Some(HeapData::String(sb))) => Some(sa.cmp(sb)),
                (Some(HeapData::Vector(va)), Some(HeapData::Vector(vb))) => {
                    let mut ord = Ordering::Equal;
                    for (ea, eb) in va.iter().zip(vb.iter()) {
                        match compare_values(ea, eb, heap, seen) {
                            Some(Ordering::Equal) => {},
                            Some(o) => {
                                ord = o;
                                break;
                            },
                            None => return None,
                        }
                    }
                    (ord == Ordering::Equal)
                        .then_some(va.len().cmp(&vb.len()))
                        .or(Some(ord))
                },
                _ => None,
            };
            seen.remove(&(*ka, *kb));
            result
        },
        _ => None,
    }
}

pub fn negative(a: &StackValue, heap: &Heap) -> RuntimeResult<StackValue> {
    match resolve(a, heap) {
        Resolved::Num(p) => Ok(StackValue::Primitive(-p)),
        _ => Err(RuntimeError::type_error(
            "unsupported operand types for unary -",
        )),
    }
}

#[must_use]
pub fn not_op(a: &StackValue, heap: &Heap) -> StackValue {
    StackValue::Primitive(Primitive::Bool(!a.is_truthy(heap)))
}

fn integer_index(sv: &StackValue, heap: &Heap) -> RuntimeResult<i64> {
    match resolve(sv, heap) {
        Resolved::Num(Primitive::Integer(i)) => Ok(i),
        _ => Err(RuntimeError::type_error(
            "index to a container must be an integer",
        )),
    }
}

fn heap_key(container: &StackValue) -> RuntimeResult<DefaultKey> {
    match container {
        StackValue::Heap(key) => Ok(*key),
        _ => Err(RuntimeError::type_error("cannot index non-container type")),
    }
}

pub fn index(
    container: &StackValue,
    index: &StackValue,
    heap: &mut Heap,
) -> RuntimeResult<StackValue> {
    let Ok(i) = usize::try_from(integer_index(index, heap)?) else {
        return Err(RuntimeError::value("index out of bounds"));
    };
    match resolve(container, heap) {
        Resolved::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            let Some(c) = chars.get(i) else {
                return Err(RuntimeError::value("index out of bounds"));
            };
            Ok(heap.alloc_string(c.to_string()))
        },
        Resolved::Vec(v) => v
            .get(i)
            .copied()
            .ok_or_else(|| RuntimeError::value("index out of bounds")),
        Resolved::Num(p) => {
            let type_name = match p {
                Primitive::Integer(_) => "int",
                Primitive::Double(_) => "float",
                Primitive::Bool(_) => "bool",
            };
            Err(RuntimeError::type_error(format!(
                "cannot index a {type_name} value"
            )))
        },
        Resolved::Other => Err(RuntimeError::type_error(
            "cannot index a non-container value",
        )),
    }
}

pub fn index_mut<'a>(
    container: &'a mut StackValue,
    index: &StackValue,
    heap: &'a mut Heap,
) -> RuntimeResult<&'a mut StackValue> {
    let Ok(i) = usize::try_from(integer_index(index, heap)?) else {
        return Err(RuntimeError::value("index out of bounds"));
    };
    let key = heap_key(container)?;

    let Some(cell) = heap.cells.get_mut(key) else {
        return Err(RuntimeError::value("invalid heap reference"));
    };
    let Some(v) = cell.value.as_mut_vector() else {
        return Err(RuntimeError::type_error("cannot index non-vector type"));
    };
    v.get_mut(i)
        .ok_or_else(|| RuntimeError::value("index out of bounds"))
}

#[must_use]
pub fn to_owned(sv: &StackValue, heap: &Heap) -> HeapData {
    match sv {
        StackValue::Nil => HeapData::Nil,
        StackValue::Primitive(p) => HeapData::Primitive(*p),
        StackValue::Heap(key) => heap
            .cells
            .get(*key)
            .map_or_else(|| HeapData::Nil, |cell| cell.value.clone()),
    }
}

impl fmt::Display for HeapData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => write!(f, "nil"),
            Self::Primitive(p) => write!(f, "{p}"),
            Self::Function(fun) => match fun {
                Function::Builtin(b) => write!(f, "function <{}>", b.name),
                Function::Bytecode(bc) => write!(f, "function <{}>", bc.name),
            },
            Self::Vector(v) => {
                write!(f, "[")?;
                for (i, sv) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{sv}")?;
                }
                write!(f, "]")
            },
            Self::String(s) => write!(f, "\"{s}\""),
        }
    }
}
