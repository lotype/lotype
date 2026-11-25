//! JSON that is still being written.

use serde_json::{Map, Number, Value};

/// Maximum number of nested JSON containers read from a tool argument.
const MAX_NESTING: usize = 32;

/// A step into a value: an object's key or an array's index.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Seg {
    Key(String),
    Index(usize),
}

/// What the start of a JSON text says so far.
#[derive(Debug, PartialEq)]
pub(crate) struct Partial {
    pub(crate) value: Value,
    /// Where the text stops inside a string, which may have more to come.
    /// Everything else in `value` is as it will stay.
    pub(crate) cut: Option<Vec<Seg>>,
    /// Nothing is missing: the text is one whole value.
    pub(crate) whole: bool,
}

/// The text is not the start of any JSON.
#[derive(Debug, PartialEq)]
pub(crate) struct Malformed;

/// Reads the start of a JSON text, as far as it goes. `None` while there is
/// no value in it yet.
pub(crate) fn read(text: &str) -> Result<Option<Partial>, Malformed> {
    let mut reader = Reader {
        rest: text,
        path: Vec::new(),
        cut: None,
        whole: true,
    };
    let Some(value) = reader.value()? else {
        return Ok(None);
    };
    reader.space();
    if !reader.rest.is_empty() {
        return Err(Malformed);
    }
    Ok(Some(Partial {
        value,
        cut: reader.cut,
        whole: reader.whole,
    }))
}

struct Reader<'a> {
    rest: &'a str,
    path: Vec<Seg>,
    cut: Option<Vec<Seg>>,
    whole: bool,
}

impl Reader<'_> {
    fn space(&mut self) {
        self.rest = self.rest.trim_start_matches([' ', '\n', '\r', '\t']);
    }

    /// Takes `sign` if it is next.
    fn take(&mut self, sign: char) -> bool {
        let rest = self.rest.strip_prefix(sign);
        rest.map(|rest| self.rest = rest).is_some()
    }

    /// The text ended before the value did.
    fn short<T>(&mut self, so_far: T) -> Result<Option<T>, Malformed> {
        self.whole = false;
        Ok(Some(so_far))
    }

    /// The next value. `None` where the text ends before there is any of it.
    fn value(&mut self) -> Result<Option<Value>, Malformed> {
        self.space();
        match self.rest.chars().next() {
            None => {
                self.whole = false;
                Ok(None)
            }
            Some('{' | '[') if self.path.len() >= MAX_NESTING => Err(Malformed),
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => {
                let (text, closed) = self.string()?;
                if !closed {
                    self.cut = Some(self.path.clone());
                    self.whole = false;
                }
                Ok(Some(Value::String(text)))
            }
            Some(_) => self.scalar(),
        }
    }

    fn object(&mut self) -> Result<Option<Value>, Malformed> {
        self.rest = &self.rest[1..];
        let mut object = Map::new();
        loop {
            self.space();
            if self.take('}') {
                return Ok(Some(Value::Object(object)));
            }
            if self.rest.is_empty() {
                return self.short(Value::Object(object));
            }
            if !self.rest.starts_with('"') {
                return Err(Malformed);
            }
            let (key, closed) = self.string()?;
            self.space();
            if !closed || self.rest.is_empty() {
                return self.short(Value::Object(object));
            }
            if !self.take(':') {
                return Err(Malformed);
            }
            self.path.push(Seg::Key(key.clone()));
            let value = self.value()?;
            self.path.pop();
            let Some(value) = value else {
                return self.short(Value::Object(object));
            };
            object.insert(key, value);
            self.space();
            if !self.take(',') && !self.rest.starts_with('}') && !self.rest.is_empty() {
                return Err(Malformed);
            }
        }
    }

    fn array(&mut self) -> Result<Option<Value>, Malformed> {
        self.rest = &self.rest[1..];
        let mut array = Vec::new();
        loop {
            self.space();
            if self.take(']') {
                return Ok(Some(Value::Array(array)));
            }
            self.path.push(Seg::Index(array.len()));
            let value = self.value()?;
            self.path.pop();
            let Some(value) = value else {
                return self.short(Value::Array(array));
            };
            array.push(value);
            self.space();
            if !self.take(',') && !self.rest.starts_with(']') && !self.rest.is_empty() {
                return Err(Malformed);
            }
        }
    }

    /// A string, and whether its closing quote is there. One that stops
    /// short is what of it is certain: half an escape is left out.
    fn string(&mut self) -> Result<(String, bool), Malformed> {
        let mut text = String::new();
        let mut chars = self.rest[1..].chars();
        loop {
            let stop = |text| Ok((text, false));
            match chars.next() {
                None => {
                    self.rest = "";
                    return stop(text);
                }
                Some('"') => {
                    self.rest = chars.as_str();
                    return Ok((text, true));
                }
                Some('\\') => match escape(&mut chars)? {
                    Some(escaped) => text.push(escaped),
                    None => {
                        self.rest = "";
                        return stop(text);
                    }
                },
                Some(control) if control < ' ' => return Err(Malformed),
                Some(other) => text.push(other),
            }
        }
    }

    /// A number or a word. One the text ends in may have more to come, so
    /// it is no value yet.
    fn scalar(&mut self) -> Result<Option<Value>, Malformed> {
        let end = self.rest.find([',', '}', ']', ' ', '\n', '\r', '\t']);
        let Some(end) = end else {
            let start = |word: &str| word.starts_with(self.rest);
            let number = |c: char| c.is_ascii_digit() || "+-.eE".contains(c);
            if !["true", "false", "null"].into_iter().any(start) && !self.rest.chars().all(number) {
                return Err(Malformed);
            }
            self.rest = "";
            self.whole = false;
            return Ok(None);
        };
        let (word, rest) = self.rest.split_at(end);
        self.rest = rest;
        Ok(Some(match word {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "null" => Value::Null,
            number => Value::Number(number.parse::<Number>().map_err(|_| Malformed)?),
        }))
    }
}

/// The character an escape stands for, after its backslash. `None` where
/// the text ends before the escape does.
fn escape(chars: &mut std::str::Chars) -> Result<Option<char>, Malformed> {
    let hex = |chars: &mut std::str::Chars| -> Result<Option<u32>, Malformed> {
        let digits = chars.as_str().get(..4);
        let Some(digits) = digits else {
            // Fewer than four are left: the text ends in them.
            return match chars.as_str().chars().all(|c| c.is_ascii_hexdigit()) {
                true => Ok(None),
                false => Err(Malformed),
            };
        };
        let unit = u32::from_str_radix(digits, 16).map_err(|_| Malformed)?;
        *chars = chars.as_str()[4..].chars();
        Ok(Some(unit))
    };
    Ok(Some(match chars.next() {
        None => return Ok(None),
        Some('n') => '\n',
        Some('t') => '\t',
        Some('r') => '\r',
        Some('b') => '\u{8}',
        Some('f') => '\u{c}',
        Some(same @ ('"' | '\\' | '/')) => same,
        Some('u') => {
            let Some(unit) = hex(chars)? else {
                return Ok(None);
            };
            // A character beyond the first plane is two escapes.
            if !(0xD800..0xDC00).contains(&unit) {
                return char::from_u32(unit).map(Some).ok_or(Malformed);
            }
            let low = match (chars.next(), chars.next()) {
                (Some('\\'), Some('u')) => hex(chars)?,
                (None, _) | (Some('\\'), None) => None,
                _ => return Err(Malformed),
            };
            let Some(low) = low else {
                return Ok(None);
            };
            let low = low.checked_sub(0xDC00).filter(|low| *low < 0x400);
            let point = 0x10000 + ((unit - 0xD800) << 10) + low.ok_or(Malformed)?;
            char::from_u32(point).ok_or(Malformed)?
        }
        Some(_) => return Err(Malformed),
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn key(name: &str) -> Seg {
        Seg::Key(name.to_owned())
    }

    #[test]
    fn every_start_of_a_text_reads_as_what_is_certain_of_it() {
        let text = r#"{"id": "7@3", "n": [1, -2.5e3, true, null], "block": "a \"q\" é 😀\n"}"#;
        let whole: Value = serde_json::from_str(text).unwrap();
        for end in (0..=text.len()).filter(|end| text.is_char_boundary(*end)) {
            let read = read(&text[..end]).unwrap_or_else(|_| panic!("at {end}"));
            let Some(partial) = read else {
                assert!(text[..end].trim().is_empty());
                continue;
            };
            assert_eq!(partial.whole, end == text.len(), "at {end}");
            // What is there is the start of what will be.
            let (Value::Object(now), Value::Object(then)) = (&partial.value, &whole) else {
                panic!("an object");
            };
            for (name, value) in now {
                let cut = partial.cut.as_deref() == Some(&[key(name)]);
                match (value, &then[name]) {
                    (Value::String(now), Value::String(then)) if cut => {
                        assert!(then.starts_with(now.as_str()), "at {end}")
                    }
                    (Value::Array(now), Value::Array(then)) => assert_eq!(now, &then[..now.len()]),
                    (now, then) => assert_eq!(now, then, "at {end}"),
                }
            }
        }
        assert_eq!(read(text).unwrap().unwrap().value, whole);
    }

    #[test]
    fn a_string_that_stops_short_is_found_where_it_is() {
        let partial = read(r#"{"blocks": ["one", "tw"#).unwrap().unwrap();
        assert_eq!(partial.value, json!({"blocks": ["one", "tw"]}));
        assert_eq!(partial.cut, Some(vec![key("blocks"), Seg::Index(1)]));
        // A key that stops short is no part of the value yet.
        let partial = read(r#"{"id": "1", "blo"#).unwrap().unwrap();
        assert_eq!((partial.value, partial.cut), (json!({"id": "1"}), None));
    }

    #[test]
    fn what_is_no_json_is_refused() {
        for text in [
            "x",
            "{]",
            r#"{"a" 1}"#,
            r#"{"a": 1 "b""#,
            "[1 2]",
            r#""\q""#,
            "{} {}",
            "tru!",
            r#""\ud83dx""#,
        ] {
            assert_eq!(read(text), Err(Malformed), "{text}");
        }
    }

    #[test]
    fn container_depth_is_bounded_for_both_complete_and_partial_json() {
        let open = "[".repeat(MAX_NESTING);
        let close = "]".repeat(MAX_NESTING);
        assert!(read(&format!("{open}null{close}")).unwrap().unwrap().whole);
        assert!(!read(&open).unwrap().unwrap().whole);
        assert_eq!(read(&format!("{open}[")), Err(Malformed));
        let object = r#"{"value":"#.repeat(MAX_NESTING);
        assert_eq!(read(&format!("{object}{{")), Err(Malformed));
    }
}
