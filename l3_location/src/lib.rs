use std::fmt;

pub type Counter = usize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub filename: Option<String>,
    pub line: Counter,
    pub column: Counter,
}

impl Position {
    #[must_use]
    pub fn new(filename: Option<String>, line: Counter, column: Counter) -> Self {
        Self {
            filename,
            line,
            column,
        }
    }

    pub fn lines(&mut self, count: Counter) {
        if count > 0 {
            self.line = add(self.line, count, 1);
            self.column = 1;
        }
    }

    pub fn columns(&mut self, count: Counter) {
        self.column = add(self.column, count, 1);
    }
}

impl Default for Position {
    fn default() -> Self {
        Self {
            filename: None,
            line: 1,
            column: 1,
        }
    }
}

fn add(lhs: Counter, rhs: Counter, min: Counter) -> Counter {
    lhs.saturating_add(rhs).max(min)
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref filename) = self.filename {
            write!(f, "{}:{}.{}", filename, self.line, self.column)
        } else {
            write!(f, "{}.{}", self.line, self.column)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location {
    pub begin: Position,
    pub end: Position,
}

impl Location {
    #[must_use]
    pub fn new(begin: Position, end: Position) -> Self {
        Self { begin, end }
    }

    pub fn step(&mut self) {
        self.begin = self.end.clone();
    }

    pub fn columns(&mut self, count: Counter) {
        self.end.columns(count);
    }

    pub fn lines(&mut self, count: Counter) {
        self.end.lines(count);
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.begin)?;
        if self.begin == self.end {
            return Ok(());
        }
        if self.begin.line == self.end.line {
            write!(f, "-{}", self.end.column)
        } else {
            write!(f, "-{}.{}", self.end.line, self.end.column)
        }
    }
}
