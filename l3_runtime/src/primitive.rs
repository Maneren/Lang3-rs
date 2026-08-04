use std::{
    cmp::Ordering,
    fmt,
    ops::{Add, Div, Mul, Neg, Rem, Sub},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Primitive {
    Bool(bool),
    Integer(i64),
    Double(f64),
}

impl Primitive {
    #[must_use]
    pub const fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    #[must_use]
    pub const fn is_integer(&self) -> bool {
        matches!(self, Self::Integer(_))
    }

    #[must_use]
    pub const fn is_double(&self) -> bool {
        matches!(self, Self::Double(_))
    }

    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(b) = self {
            Some(*b)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn as_integer(&self) -> Option<i64> {
        if let Self::Integer(i) = self {
            Some(*i)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn as_double(&self) -> Option<f64> {
        if let Self::Double(f) = self {
            Some(*f)
        } else {
            None
        }
    }

    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Integer(i) => *i != 0,
            Self::Double(f) => *f != 0.0,
        }
    }

    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Integer(_) => "int",
            Self::Double(_) => "double",
        }
    }
}

impl fmt::Display for Primitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Integer(i) => write!(f, "{i}"),
            Self::Double(d) => write!(f, "{d}"),
        }
    }
}

// Arithmetic operations

impl Add for Primitive {
    type Output = Result<Self, &'static str>;
    fn add(self, rhs: Self) -> Result<Self, &'static str> {
        match (self, rhs) {
            (Self::Integer(a), Self::Integer(b)) => Ok(Self::Integer(a.wrapping_add(b))),
            (Self::Double(a), Self::Double(b)) => Ok(Self::Double(a + b)),
            (Self::Integer(a), Self::Double(b)) => Ok(Self::Double(a as f64 + b)),
            (Self::Double(a), Self::Integer(b)) => Ok(Self::Double(a + b as f64)),
            _ => Err("unsupported operand types for +"),
        }
    }
}

impl Sub for Primitive {
    type Output = Result<Self, &'static str>;
    fn sub(self, rhs: Self) -> Result<Self, &'static str> {
        match (self, rhs) {
            (Self::Integer(a), Self::Integer(b)) => Ok(Self::Integer(a.wrapping_sub(b))),
            (Self::Double(a), Self::Double(b)) => Ok(Self::Double(a - b)),
            (Self::Integer(a), Self::Double(b)) => Ok(Self::Double(a as f64 - b)),
            (Self::Double(a), Self::Integer(b)) => Ok(Self::Double(a - b as f64)),
            _ => Err("unsupported operand types for -"),
        }
    }
}

impl Mul for Primitive {
    type Output = Result<Self, &'static str>;
    fn mul(self, rhs: Self) -> Result<Self, &'static str> {
        match (self, rhs) {
            (Self::Integer(a), Self::Integer(b)) => Ok(Self::Integer(a.wrapping_mul(b))),
            (Self::Double(a), Self::Double(b)) => Ok(Self::Double(a * b)),
            (Self::Integer(a), Self::Double(b)) => Ok(Self::Double(a as f64 * b)),
            (Self::Double(a), Self::Integer(b)) => Ok(Self::Double(a * b as f64)),
            _ => Err("unsupported operand types for *"),
        }
    }
}

impl Div for Primitive {
    type Output = Result<Self, &'static str>;
    fn div(self, rhs: Self) -> Result<Self, &'static str> {
        match (self, rhs) {
            (Self::Integer(_), Self::Integer(0)) => Err("division by zero"),
            (Self::Integer(a), Self::Integer(b)) => Ok(Self::Integer(a.wrapping_div(b))),
            (Self::Double(a), Self::Double(b)) => Ok(Self::Double(a / b)),
            (Self::Integer(a), Self::Double(b)) => Ok(Self::Double(a as f64 / b)),
            (Self::Double(a), Self::Integer(b)) => Ok(Self::Double(a / b as f64)),
            _ => Err("unsupported operand types for /"),
        }
    }
}

impl Rem for Primitive {
    type Output = Result<Self, &'static str>;
    fn rem(self, rhs: Self) -> Result<Self, &'static str> {
        match (self, rhs) {
            (Self::Integer(_), Self::Integer(0)) => Err("modulo by zero"),
            (Self::Integer(a), Self::Integer(b)) => Ok(Self::Integer(a.wrapping_rem(b))),
            (Self::Double(a), Self::Double(b)) => Ok(Self::Double(a % b)),
            _ => Err("unsupported operand types for %"),
        }
    }
}

impl Neg for Primitive {
    type Output = Self;
    fn neg(self) -> Self {
        match self {
            Self::Integer(i) => Self::Integer(i.wrapping_neg()),
            Self::Double(f) => Self::Double(-f),
            Self::Bool(b) => Self::Integer(if b { -1 } else { 0 }),
        }
    }
}

/// Three-way comparison between primitives. Cross-type compares promote integer
/// to double.
#[must_use]
pub fn compare_primitives(a: Primitive, b: Primitive) -> Option<Ordering> {
    match (a, b) {
        (Primitive::Integer(a), Primitive::Integer(b)) => Some(a.cmp(&b)),
        (Primitive::Double(a), Primitive::Double(b)) => a.partial_cmp(&b),
        (Primitive::Bool(a), Primitive::Bool(b)) => Some(a.cmp(&b)),
        (Primitive::Integer(a), Primitive::Double(b)) => (a as f64).partial_cmp(&b),
        (Primitive::Double(a), Primitive::Integer(b)) => a.partial_cmp(&(b as f64)),
        (Primitive::Bool(b), Primitive::Integer(i)) => Some(i64::from(b).cmp(&i)),
        (Primitive::Integer(i), Primitive::Bool(b)) => Some(i.cmp(&i64::from(b))),
        (Primitive::Bool(b), Primitive::Double(d)) => (i64::from(b) as f64).partial_cmp(&d),
        (Primitive::Double(d), Primitive::Bool(b)) => d.partial_cmp(&(i64::from(b) as f64)),
    }
}
