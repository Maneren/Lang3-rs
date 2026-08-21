use std::{cmp::Ordering, fmt};

use slotmap::DefaultKey;

use crate::{
    error::{RuntimeError, RuntimeResult},
    function::Function,
    heap::Heap,
    primitive::{PowError, Primitive, compare_primitives},
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
    #[inline]
    #[must_use]
    pub const fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }

    #[inline]
    #[must_use]
    pub const fn is_primitive(&self) -> bool {
        matches!(self, Self::Primitive(_))
    }

    #[inline]
    #[must_use]
    pub const fn is_function(&self) -> bool {
        matches!(self, Self::Function(_))
    }

    #[inline]
    #[must_use]
    pub const fn is_vector(&self) -> bool {
        matches!(self, Self::Vector(_))
    }

    #[inline]
    #[must_use]
    pub const fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    #[must_use]
    #[inline]
    pub const fn as_primitive(&self) -> Option<Primitive> {
        if let Self::Primitive(p) = self {
            Some(*p)
        } else {
            None
        }
    }

    #[inline]
    pub const fn as_mut_primitive(&mut self) -> Option<&mut Primitive> {
        if let Self::Primitive(p) = self {
            Some(p)
        } else {
            None
        }
    }

    #[inline]
    #[must_use]
    pub const fn as_vector(&self) -> Option<&Vec<StackValue>> {
        if let Self::Vector(v) = self {
            Some(v)
        } else {
            None
        }
    }

    #[inline]
    pub const fn as_mut_vector(&mut self) -> Option<&mut Vec<StackValue>> {
        if let Self::Vector(v) = self {
            Some(v)
        } else {
            None
        }
    }

    #[inline]
    #[must_use]
    pub const fn as_string(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s.as_str())
        } else {
            None
        }
    }

    #[inline]
    pub const fn as_mut_string(&mut self) -> Option<&mut String> {
        if let Self::String(s) = self {
            Some(s)
        } else {
            None
        }
    }

    #[must_use]
    #[inline]
    pub const fn as_function(&self) -> Option<&Function> {
        if let Self::Function(v) = self {
            Some(v)
        } else {
            None
        }
    }

    #[inline]
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
        self.fmt_with_heap_mode(heap, true)
    }

    fn fmt_with_heap_mode(&self, heap: &Heap, quote_strings: bool) -> String {
        match self {
            Self::Nil => "nil".to_string(),
            Self::Primitive(p) => format!("{p}"),
            Self::Function(f) => match f {
                Function::Builtin(b) => format!("<builtin {}>", b.name),
                Function::Bytecode(bc) => format!("<fn {}>", bc.name),
            },
            Self::Vector(v) => {
                let elems: Vec<String> = v
                    .iter()
                    .map(|sv| format_stack_value_mode(sv, heap, quote_strings))
                    .collect();
                format!("[{}]", elems.join(", "))
            },
            Self::String(s) => {
                if quote_strings {
                    format!("\"{s}\"")
                } else {
                    s.clone()
                }
            },
        }
    }
}

/// The one value formatter. `quote_strings` selects between the display form
/// (`"a"`) and the stringify form (`a`, used by `str()` and printing).
fn format_stack_value_mode(sv: &StackValue, heap: &Heap, quote_strings: bool) -> String {
    match sv {
        StackValue::Nil => "nil".to_string(),
        StackValue::Primitive(p) => format!("{p}"),
        StackValue::Heap(key) => heap.cells.get(*key).map_or_else(
            || "<dead>".to_string(),
            |cell| cell.value.fmt_with_heap_mode(heap, quote_strings),
        ),
    }
}

/// Display a value: strings are quoted, matching the constant-pool / debug
/// output.
#[must_use]
pub fn format_stack_value(sv: &StackValue, heap: &Heap) -> String {
    format_stack_value_mode(sv, heap, true)
}

/// Stringify a value: strings are emitted raw, matching `str()` semantics.
#[must_use]
pub fn stringify_stack_value(sv: &StackValue, heap: &Heap) -> String {
    format_stack_value_mode(sv, heap, false)
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
#[inline]
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

#[inline]
fn numeric_result(r: Result<Primitive, &'static str>) -> RuntimeResult<StackValue> {
    r.map(StackValue::Primitive)
        .map_err(RuntimeError::type_error)
}

#[inline]
pub fn add(a: &StackValue, b: &StackValue, heap: &mut Heap) -> RuntimeResult<StackValue> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa + pb),
        (Resolved::Str(sa), Resolved::Str(sb)) => {
            let mut s = String::with_capacity(sa.len() + sb.len());
            s.push_str(sa);
            s.push_str(sb);
            Ok(heap.alloc_string(s))
        },
        (Resolved::Vec(va), Resolved::Vec(vb)) => {
            let mut result = Vec::with_capacity(va.len() + vb.len());
            result.extend_from_slice(va);
            result.extend_from_slice(vb);
            Ok(heap.alloc_vector(result))
        },
        _ => Err(RuntimeError::type_error("unsupported operand types for +")),
    }
}

#[inline]
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

#[inline]
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

#[inline]
pub fn div(a: &StackValue, b: &StackValue, heap: &mut Heap) -> RuntimeResult<StackValue> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa / pb),
        _ => Err(RuntimeError::type_error("unsupported operand types for /")),
    }
}

#[inline]
pub fn modulo(a: &StackValue, b: &StackValue, heap: &mut Heap) -> RuntimeResult<StackValue> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => numeric_result(pa % pb),
        _ => Err(RuntimeError::type_error("unsupported operand types for %")),
    }
}

#[inline]
pub fn pow(a: &StackValue, b: &StackValue, heap: &mut Heap) -> RuntimeResult<StackValue> {
    match (resolve(a, heap), resolve(b, heap)) {
        (Resolved::Num(pa), Resolved::Num(pb)) => {
            pa.pow(pb).map(StackValue::Primitive).map_err(|e| match e {
                PowError::ExponentTooLarge(exp) => RuntimeError::value(format!(
                    "exponent must be a non-negative 32-bit integer, got {exp}"
                )),
                PowError::Unsupported => {
                    RuntimeError::type_error("unsupported operand types for ^")
                },
            })
        },
        _ => Err(RuntimeError::type_error("unsupported operand types for ^")),
    }
}

#[must_use]
#[inline]
pub fn compare(a: &StackValue, b: &StackValue, heap: &Heap) -> Option<Ordering> {
    match (a, b) {
        (StackValue::Primitive(pa), StackValue::Primitive(pb)) => compare_primitives(*pa, *pb),
        (StackValue::Nil, StackValue::Nil) => Some(Ordering::Equal),
        (StackValue::Heap(ka), StackValue::Heap(kb)) if *ka == *kb => Some(Ordering::Equal),
        (StackValue::Heap(ka), StackValue::Heap(kb)) => match (
            heap.cells.get(*ka).map(|c| &c.value),
            heap.cells.get(*kb).map(|c| &c.value),
        ) {
            (Some(HeapData::String(sa)), Some(HeapData::String(sb))) => Some(sa.cmp(sb)),
            (Some(HeapData::Vector(va)), Some(HeapData::Vector(vb))) => {
                compare_vectors(va, vb, heap, &mut Vec::new())
            },
            _ => None,
        },
        _ => None,
    }
}

/// Element-wise comparison of vectors (cycle-safe). Only this path needs the
/// `seen` stack, created lazily at the first vector-vs-vector compare.
fn compare_vectors(
    va: &[StackValue],
    vb: &[StackValue],
    heap: &Heap,
    seen: &mut Vec<(DefaultKey, DefaultKey)>,
) -> Option<Ordering> {
    let ord = Ordering::Equal;
    for (ea, eb) in va.iter().zip(vb.iter()) {
        match compare_values(ea, eb, heap, seen) {
            Some(Ordering::Equal) => {},
            Some(o) => return Some(o),
            None => return None,
        }
    }
    (ord == Ordering::Equal)
        .then_some(va.len().cmp(&vb.len()))
        .or(Some(ord))
}

/// Recursive content comparison with cycle detection via `seen`.
fn compare_values(
    a: &StackValue,
    b: &StackValue,
    heap: &Heap,
    seen: &mut Vec<(DefaultKey, DefaultKey)>,
) -> Option<Ordering> {
    match (a, b) {
        (StackValue::Primitive(pa), StackValue::Primitive(pb)) => compare_primitives(*pa, *pb),
        (StackValue::Nil, StackValue::Nil) => Some(Ordering::Equal),
        (StackValue::Heap(ka), StackValue::Heap(kb)) => {
            if ka == kb {
                return Some(Ordering::Equal);
            }
            // Revisiting a pair means the structure is cyclic; assume equal.
            if seen.contains(&(*ka, *kb)) {
                return Some(Ordering::Equal);
            }
            seen.push((*ka, *kb));
            let result = match (
                heap.cells.get(*ka).map(|c| &c.value),
                heap.cells.get(*kb).map(|c| &c.value),
            ) {
                (Some(HeapData::String(sa)), Some(HeapData::String(sb))) => Some(sa.cmp(sb)),
                (Some(HeapData::Vector(va)), Some(HeapData::Vector(vb))) => {
                    compare_vectors(va, vb, heap, seen)
                },
                _ => None,
            };
            seen.pop();
            result
        },
        _ => None,
    }
}

#[inline]
pub fn negative(a: &StackValue, heap: &Heap) -> RuntimeResult<StackValue> {
    match resolve(a, heap) {
        Resolved::Num(p) => Ok(StackValue::Primitive(-p)),
        _ => Err(RuntimeError::type_error(
            "unsupported operand types for unary -",
        )),
    }
}

#[inline]
#[must_use]
pub fn not_op(a: &StackValue, heap: &Heap) -> StackValue {
    StackValue::Primitive(Primitive::Bool(!a.is_truthy(heap)))
}

#[inline]
fn integer_index(sv: &StackValue, heap: &Heap) -> RuntimeResult<i64> {
    match resolve(sv, heap) {
        Resolved::Num(Primitive::Integer(i)) => Ok(i),
        _ => Err(RuntimeError::type_error(
            "index to a container must be an integer",
        )),
    }
}

#[inline]
fn heap_key(container: &StackValue) -> RuntimeResult<DefaultKey> {
    match container {
        StackValue::Heap(key) => Ok(*key),
        _ => Err(RuntimeError::type_error("cannot index non-container type")),
    }
}

#[inline]
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
            let Some(c) = s.chars().nth(i) else {
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

#[inline]
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
#[inline]
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
