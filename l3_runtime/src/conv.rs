use crate::{
    error::{RuntimeError, RuntimeResult},
    heap::Heap,
    heap_data::HeapData,
    primitive::Primitive,
    stack_value::StackValue,
};

/// Strictly extract an integer from a stack value (only `Integer` primitives).
#[inline]
#[must_use]
pub const fn as_integer(sv: &StackValue) -> Option<i64> {
    match sv {
        StackValue::Primitive(p) => p.as_integer(),
        _ => None,
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "int() truncates doubles toward zero like other languages"
)]
fn primitive_to_integer(p: Primitive) -> RuntimeResult<i64> {
    match p {
        Primitive::Integer(i) => Ok(i),
        Primitive::Bool(b) => Ok(i64::from(b)),
        Primitive::Double(f) => {
            if f.is_finite() {
                Ok(f as i64)
            } else {
                Err(RuntimeError::value(format!("cannot convert {f} to int")))
            }
        },
    }
}

/// Coercive conversion to an integer (the `int` builtin).
///
/// Doubles truncate toward zero (saturating like Rust's `as`), strings parse,
/// and anything unconvertible is a runtime error.
#[inline]
pub fn to_integer(sv: &StackValue, heap: &Heap) -> RuntimeResult<i64> {
    match sv {
        StackValue::Primitive(p) => primitive_to_integer(*p),
        StackValue::Nil => Err(RuntimeError::type_error("cannot convert nil to int")),
        StackValue::Heap(key) => heap.cells.get(*key).map_or_else(
            || Err(RuntimeError::value("invalid heap reference")),
            |cell| match &cell.value {
                HeapData::String(s) => s
                    .trim()
                    .parse()
                    .ok()
                    .ok_or_else(|| RuntimeError::value(format!("cannot convert {s:?} to int"))),
                _ => Err(RuntimeError::type_error(format!(
                    "cannot convert a {} to int",
                    cell.value.type_name(heap)
                ))),
            },
        ),
    }
}

/// Convert an integer count to a `usize`, rejecting negative counts.
#[inline]
pub fn non_negative_count(n: i64) -> RuntimeResult<usize> {
    usize::try_from(n)
        .ok()
        .ok_or_else(|| RuntimeError::value("count must be non-negative"))
}
