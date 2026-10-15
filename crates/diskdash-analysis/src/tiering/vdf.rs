//! A small reader for Valve's text KeyValues format (`libraryfolders.vdf`,
//! `appmanifest_*.acf`). Read-only; malformed input is an error, never a panic.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vdf {
    Str(String),
    Obj(Vec<(String, Vdf)>),
}

impl Vdf {
    /// First child with this key (case-insensitive).
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Obj(items) => items.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Vdf::Str(s) => Some(s),
            Vdf::Obj(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Obj(items) => items,
            Vdf::Str(_) => &[],
        }
    }
}

struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

#[derive(Debug, PartialEq, Eq)]
enum Tok {
    Str(String),
    Open,
    Close,
}

impl<'a> Lexer<'a> {
    fn next(&mut self) -> Result<Option<Tok>, String> {
        loop {
            match self.chars.peek().copied() {
                None => return Ok(None),
                Some(c) if c.is_whitespace() => {
                    self.chars.next();
                }
                Some('/') => {
                    self.chars.next();
                    if self.chars.next() != Some('/') {
                        return Err("unexpected `/`".into());
                    }
                    for c in self.chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                Some('{') => {
                    self.chars.next();
                    return Ok(Some(Tok::Open));
                }
                Some('}') => {
                    self.chars.next();
                    return Ok(Some(Tok::Close));
                }
                Some('"') => {
                    self.chars.next();
                    let mut s = String::new();
                    loop {
                        match self.chars.next() {
                            None => return Err("unterminated string".into()),
                            Some('"') => return Ok(Some(Tok::Str(s))),
                            Some('\\') => match self.chars.next() {
                                Some('n') => s.push('\n'),
                                Some('t') => s.push('\t'),
                                Some('\\') => s.push('\\'),
                                Some('"') => s.push('"'),
                                Some(c) => {
                                    s.push('\\');
                                    s.push(c);
                                }
                                None => return Err("unterminated string".into()),
                            },
                            Some(c) => s.push(c),
                        }
                    }
                }
                Some(c) => return Err(format!("unexpected character `{c}`")),
            }
        }
    }
}

fn parse_pairs(lex: &mut Lexer<'_>, depth: usize, top: bool) -> Result<Vec<(String, Vdf)>, String> {
    if depth > 32 {
        return Err("nested too deeply".into());
    }
    let mut items = Vec::new();
    loop {
        match lex.next()? {
            None if top => return Ok(items),
            None => return Err("unexpected end of file (missing `}`)".into()),
            Some(Tok::Close) if !top => return Ok(items),
            Some(Tok::Close) => return Err("unexpected `}`".into()),
            Some(Tok::Open) => return Err("`{` without a key".into()),
            Some(Tok::Str(key)) => match lex.next()? {
                Some(Tok::Str(v)) => items.push((key, Vdf::Str(v))),
                Some(Tok::Open) => items.push((key, Vdf::Obj(parse_pairs(lex, depth + 1, false)?))),
                _ => return Err(format!("key `{key}` has no value")),
            },
        }
    }
}

pub fn parse(src: &str) -> Result<Vdf, String> {
    let mut lex = Lexer { chars: src.trim_start_matches('\u{FEFF}').chars().peekable() };
    Ok(Vdf::Obj(parse_pairs(&mut lex, 0, true)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_objects_comments_and_escapes() {
        let v = parse("// header\n\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"apps\" { \"220\" \"1\" }\n\t}\n}\n").unwrap();
        let lib = v.get("libraryfolders").unwrap().get("0").unwrap();
        assert_eq!(lib.str("path"), Some(r"C:\Program Files (x86)\Steam"));
        assert_eq!(lib.get("apps").unwrap().entries().len(), 1);
        assert!(v.get("LIBRARYFOLDERS").is_some(), "keys are case-insensitive");
    }

    #[test]
    fn malformed_input_is_an_error() {
        for bad in ["\"a\" {", "\"a\" { \"b\" }", "}", "{ \"a\" \"b\" }", "\"unterminated", "bare words", "\"a\""] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert!(parse("").unwrap().entries().is_empty());
        assert!(parse(&"\"a\" {".repeat(100)).is_err(), "deep nesting is refused");
    }
}
