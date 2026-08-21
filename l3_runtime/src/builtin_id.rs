use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BuiltinId {
    Print = 0,
    Println,
    Assert,
    Error,
    Int,
    Str,
    Len,
    Head,
    Tail,
    Drop,
    Take,
    Range,
    Id,
    Map,
    Count,
    Random,
    Input,
    Sleep,
    Sum,
}

impl BuiltinId {
    pub const COUNT: usize = 19;

    pub const ALL: [Self; Self::COUNT] = [
        Self::Print,
        Self::Println,
        Self::Assert,
        Self::Error,
        Self::Int,
        Self::Str,
        Self::Len,
        Self::Head,
        Self::Tail,
        Self::Drop,
        Self::Take,
        Self::Range,
        Self::Id,
        Self::Map,
        Self::Count,
        Self::Random,
        Self::Input,
        Self::Sleep,
        Self::Sum,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Print => "print",
            Self::Println => "println",
            Self::Assert => "assert",
            Self::Error => "error",
            Self::Int => "int",
            Self::Str => "str",
            Self::Len => "len",
            Self::Head => "head",
            Self::Tail => "tail",
            Self::Drop => "drop",
            Self::Take => "take",
            Self::Range => "range",
            Self::Id => "id",
            Self::Map => "map",
            Self::Count => "count",
            Self::Random => "random",
            Self::Input => "input",
            Self::Sleep => "sleep",
            Self::Sum => "sum",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "print" => Some(Self::Print),
            "println" => Some(Self::Println),
            "assert" => Some(Self::Assert),
            "error" => Some(Self::Error),
            "int" => Some(Self::Int),
            "str" => Some(Self::Str),
            "len" => Some(Self::Len),
            "head" => Some(Self::Head),
            "tail" => Some(Self::Tail),
            "drop" => Some(Self::Drop),
            "take" => Some(Self::Take),
            "range" => Some(Self::Range),
            "id" => Some(Self::Id),
            "map" => Some(Self::Map),
            "count" => Some(Self::Count),
            "random" => Some(Self::Random),
            "input" => Some(Self::Input),
            "sleep" => Some(Self::Sleep),
            "sum" => Some(Self::Sum),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_usize(self) -> usize {
        self as usize
    }
}

impl fmt::Display for BuiltinId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}
