use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Primitive {
    Bool(bool),
    Integer(i64),
    Double(f64),
}

impl Hash for Primitive {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Primitive::Bool(b) => {
                0u8.hash(state);
                b.hash(state);
            }
            Primitive::Integer(i) => {
                1u8.hash(state);
                i.hash(state);
            }
            Primitive::Double(d) => {
                2u8.hash(state);
                d.to_bits().hash(state);
            }
        }
    }
}

impl Primitive {
    #[must_use]
    pub fn is_bool(&self) -> bool {
        matches!(self, Primitive::Bool(_))
    }

    #[must_use]
    pub fn is_integer(&self) -> bool {
        matches!(self, Primitive::Integer(_))
    }

    #[must_use]
    pub fn is_double(&self) -> bool {
        matches!(self, Primitive::Double(_))
    }

    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        if let Primitive::Bool(b) = self {
            Some(*b)
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_integer(&self) -> Option<i64> {
        if let Primitive::Integer(i) = self {
            Some(*i)
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_double(&self) -> Option<f64> {
        if let Primitive::Double(f) = self {
            Some(*f)
        } else {
            None
        }
    }

    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            Primitive::Bool(b) => *b,
            Primitive::Integer(i) => *i != 0,
            Primitive::Double(f) => *f != 0.0,
        }
    }

    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Primitive::Bool(_) => "bool",
            Primitive::Integer(_) => "int",
            Primitive::Double(_) => "double",
        }
    }
}

impl fmt::Display for Primitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Primitive::Bool(b) => write!(f, "{b}"),
            Primitive::Integer(i) => write!(f, "{i}"),
            Primitive::Double(d) => write!(f, "{d}"),
        }
    }
}

// Arithmetic operations

impl std::ops::Add for Primitive {
    type Output = Result<Primitive, &'static str>;
    fn add(self, rhs: Primitive) -> Result<Primitive, &'static str> {
        match (self, rhs) {
            (Primitive::Integer(a), Primitive::Integer(b)) => {
                Ok(Primitive::Integer(a.wrapping_add(b)))
            }
            (Primitive::Double(a), Primitive::Double(b)) => Ok(Primitive::Double(a + b)),
            (Primitive::Integer(a), Primitive::Double(b)) => Ok(Primitive::Double(a as f64 + b)),
            (Primitive::Double(a), Primitive::Integer(b)) => Ok(Primitive::Double(a + b as f64)),
            _ => Err("unsupported operand types for +"),
        }
    }
}

impl std::ops::Sub for Primitive {
    type Output = Result<Primitive, &'static str>;
    fn sub(self, rhs: Primitive) -> Result<Primitive, &'static str> {
        match (self, rhs) {
            (Primitive::Integer(a), Primitive::Integer(b)) => {
                Ok(Primitive::Integer(a.wrapping_sub(b)))
            }
            (Primitive::Double(a), Primitive::Double(b)) => Ok(Primitive::Double(a - b)),
            (Primitive::Integer(a), Primitive::Double(b)) => Ok(Primitive::Double(a as f64 - b)),
            (Primitive::Double(a), Primitive::Integer(b)) => Ok(Primitive::Double(a - b as f64)),
            _ => Err("unsupported operand types for -"),
        }
    }
}

impl std::ops::Mul for Primitive {
    type Output = Result<Primitive, &'static str>;
    fn mul(self, rhs: Primitive) -> Result<Primitive, &'static str> {
        match (self, rhs) {
            (Primitive::Integer(a), Primitive::Integer(b)) => {
                Ok(Primitive::Integer(a.wrapping_mul(b)))
            }
            (Primitive::Double(a), Primitive::Double(b)) => Ok(Primitive::Double(a * b)),
            (Primitive::Integer(a), Primitive::Double(b)) => Ok(Primitive::Double(a as f64 * b)),
            (Primitive::Double(a), Primitive::Integer(b)) => Ok(Primitive::Double(a * b as f64)),
            _ => Err("unsupported operand types for *"),
        }
    }
}

impl std::ops::Div for Primitive {
    type Output = Result<Primitive, &'static str>;
    fn div(self, rhs: Primitive) -> Result<Primitive, &'static str> {
        match (self, rhs) {
            (Primitive::Integer(_), Primitive::Integer(0)) => Err("division by zero"),
            (Primitive::Integer(a), Primitive::Integer(b)) => {
                Ok(Primitive::Integer(a.wrapping_div(b)))
            }
            (Primitive::Double(a), Primitive::Double(b)) => Ok(Primitive::Double(a / b)),
            (Primitive::Integer(a), Primitive::Double(b)) => Ok(Primitive::Double(a as f64 / b)),
            (Primitive::Double(a), Primitive::Integer(b)) => Ok(Primitive::Double(a / b as f64)),
            _ => Err("unsupported operand types for /"),
        }
    }
}

impl std::ops::Rem for Primitive {
    type Output = Result<Primitive, &'static str>;
    fn rem(self, rhs: Primitive) -> Result<Primitive, &'static str> {
        match (self, rhs) {
            (Primitive::Integer(_), Primitive::Integer(0)) => Err("modulo by zero"),
            (Primitive::Integer(a), Primitive::Integer(b)) => {
                Ok(Primitive::Integer(a.wrapping_rem(b)))
            }
            (Primitive::Double(a), Primitive::Double(b)) => Ok(Primitive::Double(a % b)),
            _ => Err("unsupported operand types for %"),
        }
    }
}

impl std::ops::Neg for Primitive {
    type Output = Primitive;
    fn neg(self) -> Primitive {
        match self {
            Primitive::Integer(i) => Primitive::Integer(i.wrapping_neg()),
            Primitive::Double(f) => Primitive::Double(-f),
            Primitive::Bool(b) => Primitive::Integer(if b { -1 } else { 0 }),
        }
    }
}

/// Three-way comparison between primitives. Cross-type compares promote integer to double.
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
