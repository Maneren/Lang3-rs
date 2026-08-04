use l3_location::*;

#[must_use]
pub fn byte_to_position(source: &str, byte: usize, filename: &str) -> Position {
    let mut line: usize = 1;
    let mut col: usize = 1;
    for (i, c) in source.char_indices() {
        if i >= byte {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    Position::new(Some(filename.to_string()), line, col)
}

#[must_use]
pub fn make_loc(begin: usize, end: usize, source: &str, filename: &str) -> Location {
    Location::new(
        byte_to_position(source, begin, filename),
        byte_to_position(source, end, filename),
    )
}

#[must_use]
pub fn mk_id(name: &str, loc: Location) -> l3_ast::Identifier {
    l3_ast::Identifier::new(name.to_string(), loc)
}

#[must_use]
pub fn unescape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('"') => result.push('"'),
                Some('x') => {
                    let hex: String = chars.by_ref().take(2).collect();
                    if let Ok(code) = u8::from_str_radix(&hex, 16) {
                        result.push(code as char);
                    }
                },
                Some('\\') | None => result.push('\\'),
                Some(c) => result.push(c),
            }
        } else {
            result.push(c);
        }
    }
    result
}
