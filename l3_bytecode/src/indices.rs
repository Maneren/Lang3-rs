use std::{
    fmt::{self, Display, Formatter},
    ops::{Add, Sub},
};

/// Convert a `usize` index to the typed index stored in bytecode. The target
/// type is inferred from the use site.
#[must_use]
#[inline]
pub fn idx<T>(v: usize) -> T
where
    T: TryFrom<usize>,
    T::Error: fmt::Debug,
{
    if cfg!(debug_assertions) {
        T::try_from(v).expect("indices fit in the target type")
    } else {
        // SAFETY: All indices come from the compiler that is considered
        // infallible by the VM
        unsafe { T::try_from(v).unwrap_unchecked() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct StackIndex(pub u32);

impl StackIndex {
    #[inline]
    #[must_use]
    pub const fn as_index(&self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub fn checked_sub(self, rhs: u32) -> Option<Self> {
        self.0.checked_sub(rhs).map(Self)
    }
}

impl TryFrom<usize> for StackIndex {
    type Error = ();

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u32::try_from(value).map_or(Err(()), |v| Ok(Self(v)))
    }
}

impl Add<u32> for StackIndex {
    type Output = Self;

    fn add(self, rhs: u32) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl Sub<u32> for StackIndex {
    type Output = Self;

    fn sub(self, rhs: u32) -> Self::Output {
        Self(self.0 - rhs)
    }
}

impl Add<LocalIndex> for StackIndex {
    type Output = Self;

    fn add(self, rhs: LocalIndex) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Display for StackIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ConstantIndex(pub u32);

impl ConstantIndex {
    #[inline]
    #[must_use]
    pub const fn as_index(&self) -> usize {
        self.0 as usize
    }
}

impl TryFrom<usize> for ConstantIndex {
    type Error = ();

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u32::try_from(value).map_or(Err(()), |v| Ok(Self(v)))
    }
}

impl Display for ConstantIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct LocalIndex(pub u32);

impl LocalIndex {
    #[inline]
    #[must_use]
    pub const fn as_index(&self) -> usize {
        self.0 as usize
    }
}

impl TryFrom<usize> for LocalIndex {
    type Error = ();

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u32::try_from(value).map_or(Err(()), |v| Ok(Self(v)))
    }
}

impl Sub for LocalIndex {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl Display for LocalIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct UpvalueIndex(pub u32);

impl UpvalueIndex {
    #[inline]
    #[must_use]
    pub const fn as_index(&self) -> usize {
        self.0 as usize
    }
}

impl TryFrom<usize> for UpvalueIndex {
    type Error = ();

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u32::try_from(value).map_or(Err(()), |v| Ok(Self(v)))
    }
}

impl Display for UpvalueIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ChunkId(pub u32);

impl ChunkId {
    #[inline]
    #[must_use]
    pub const fn as_index(&self) -> usize {
        self.0 as usize
    }
}

impl TryFrom<usize> for ChunkId {
    type Error = ();

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u32::try_from(value).map_or(Err(()), |v| Ok(Self(v)))
    }
}

impl Display for ChunkId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CodeOffset(pub u32);

impl CodeOffset {
    #[inline]
    #[must_use]
    pub const fn as_index(&self) -> usize {
        self.0 as usize
    }
}

impl TryFrom<usize> for CodeOffset {
    type Error = ();

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u32::try_from(value).map_or(Err(()), |v| Ok(Self(v)))
    }
}

impl Display for CodeOffset {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
