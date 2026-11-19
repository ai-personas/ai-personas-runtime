//! Ephemeral projection of the requester-facing field of an ordinary decision.
//! The collector establishes the carrier. This parser checks the invocation
//! and JSON path before exposing even a prefix of the reply. It never returns
//! a decision or participates in transport, validation, or adoption.
use std::collections::{hash_map::RandomState, HashSet};
use std::hash::{BuildHasher, DefaultHasher, Hasher};

const EXCERPT_BYTES: usize = 2048;
// Scratch budgets apply only to this optional preview. Total decision length,
// strings, arrays and the number of sequential containers have no such limit.
const OPEN_CONTAINERS: usize = 512;
const OPEN_OBJECT_KEYS: usize = 16_384;

pub(super) struct PublicReply {
    invocation: Box<str>,
    invocation_bytes: usize,
    invocation_seen: bool,
    version_seen: bool,
    cursor: Cursor,
    frames: Vec<Frame>,
    token: Token,
    field: Field,
    root_key: String,
    hash_builder: RandomState,
    key_hash: DefaultHasher,
    open_keys: usize,
    excerpt: String,
    truncated: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Cursor {
    Root,
    FirstKey,
    Key,
    Colon,
    FirstItem,
    Value,
    AfterValue,
    Done,
    Invalid,
}

enum Frame {
    Object(HashSet<u64>),
    Array,
}

enum Token {
    None,
    String(Text, Decoder),
    Literal(&'static [u8]),
    Number(Number),
    Version,
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Other,
    Version,
    Invocation,
    Reply,
}

#[derive(Clone, Copy)]
enum Text {
    Ignored,
    Key,
    Invocation,
    Reply,
}

impl PublicReply {
    pub(super) fn new(invocation: &str) -> Self {
        let hash_builder = RandomState::new();
        Self {
            invocation: invocation.into(),
            invocation_bytes: 0,
            invocation_seen: false,
            version_seen: false,
            cursor: if invocation.is_empty() {
                Cursor::Invalid
            } else {
                Cursor::Root
            },
            frames: Vec::new(),
            token: Token::None,
            field: Field::Other,
            root_key: String::new(),
            key_hash: hash_builder.build_hasher(),
            hash_builder,
            open_keys: 0,
            excerpt: String::new(),
            truncated: false,
        }
    }

    pub(super) fn current(&self) -> Option<(&str, bool)> {
        (self.cursor != Cursor::Invalid
            && self.version_seen
            && self.invocation_seen
            && !self.excerpt.is_empty())
        .then_some((&self.excerpt, self.truncated))
    }

    // A single candidate's visible text only grows, or disappears entirely.
    fn visible_size(&self) -> Option<(usize, bool)> {
        self.current()
            .map(|(text, truncated)| (text.len(), truncated))
    }

    pub(super) fn push(&mut self, fragment: impl AsRef<[u8]>) -> bool {
        if self.cursor == Cursor::Invalid {
            return false;
        }
        let before = self.visible_size();
        for &byte in fragment.as_ref() {
            if self.consume(byte).is_err() {
                self.invalidate();
                break;
            }
        }
        before != self.visible_size()
    }

    pub(super) fn finish(&mut self) -> bool {
        let before = self.visible_size();
        if self.cursor != Cursor::Done || !self.version_seen || !self.invocation_seen {
            self.invalidate();
        }
        before != self.visible_size()
    }

    fn invalidate(&mut self) {
        self.cursor = Cursor::Invalid;
        self.excerpt.clear();
        self.frames.clear();
    }

    fn consume(&mut self, byte: u8) -> Result<(), ()> {
        match &mut self.token {
            Token::String(purpose, decoder) => {
                let purpose = *purpose;
                return match decoder.push(byte)? {
                    Decoded::More => Ok(()),
                    Decoded::Char(ch) => self.character(purpose, ch),
                    Decoded::End => self.end_string(purpose),
                };
            }
            Token::Literal(rest) => {
                let (&next, tail) = rest.split_first().ok_or(())?;
                if byte != next {
                    return Err(());
                }
                if tail.is_empty() {
                    self.end_value();
                } else {
                    *rest = tail;
                }
                return Ok(());
            }
            Token::Number(number) => {
                if number.advance(byte)? {
                    return Ok(());
                }
                self.end_value();
            }
            Token::Version => {
                // A single '4' is not the integer 4 until a delimiter arrives.
                if !delimiter(byte) {
                    return Err(());
                }
                self.version_seen = true;
                self.end_value();
            }
            Token::None => {}
        }
        if whitespace(byte) {
            return Ok(());
        }
        match (self.cursor, byte) {
            (Cursor::Root, b'{') => self.open(true),
            (Cursor::FirstKey | Cursor::Key, b'"') => {
                self.root_key.clear();
                self.key_hash = self.hash_builder.build_hasher();
                self.token = Token::String(Text::Key, Decoder::Plain);
                Ok(())
            }
            (Cursor::Colon, b':') => {
                self.cursor = Cursor::Value;
                Ok(())
            }
            (Cursor::FirstKey | Cursor::AfterValue, b'}')
                if matches!(self.frames.last(), Some(Frame::Object(_))) =>
            {
                self.close()
            }
            (Cursor::FirstItem | Cursor::AfterValue, b']')
                if matches!(self.frames.last(), Some(Frame::Array)) =>
            {
                self.close()
            }
            (Cursor::FirstItem | Cursor::Value, _) => self.start_value(byte),
            (Cursor::AfterValue, b',') => {
                self.cursor = match self.frames.last().ok_or(())? {
                    Frame::Array => Cursor::Value,
                    Frame::Object(_) => Cursor::Key,
                };
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn open(&mut self, object: bool) -> Result<(), ()> {
        if self.frames.len() == OPEN_CONTAINERS {
            return Err(());
        }
        if object {
            self.frames.push(Frame::Object(HashSet::new()));
            self.cursor = Cursor::FirstKey;
        } else {
            self.frames.push(Frame::Array);
            self.cursor = Cursor::FirstItem;
        }
        Ok(())
    }

    fn close(&mut self) -> Result<(), ()> {
        if let Frame::Object(keys) = self.frames.pop().ok_or(())? {
            self.open_keys -= keys.len();
        }
        self.end_value();
        Ok(())
    }

    fn end_value(&mut self) {
        self.token = Token::None;
        self.cursor = if self.frames.is_empty() {
            Cursor::Done
        } else {
            Cursor::AfterValue
        };
    }

    fn start_value(&mut self, byte: u8) -> Result<(), ()> {
        let field = if self.frames.len() == 1 {
            self.field
        } else {
            Field::Other
        };
        self.token = match (field, byte) {
            (Field::Version, b'4') => Token::Version,
            (Field::Invocation, b'"') => Token::String(Text::Invocation, Decoder::Plain),
            (Field::Reply, b'"') => Token::String(Text::Reply, Decoder::Plain),
            (Field::Reply | Field::Other, b'n') => Token::Literal(b"ull"),
            (Field::Other, b'"') => Token::String(Text::Ignored, Decoder::Plain),
            (Field::Other, b'{') => return self.open(true),
            (Field::Other, b'[') => return self.open(false),
            (Field::Other, b't') => Token::Literal(b"rue"),
            (Field::Other, b'f') => Token::Literal(b"alse"),
            (Field::Other, b'-') => Token::Number(Number::Sign),
            (Field::Other, b'0') => Token::Number(Number::Zero),
            (Field::Other, b'1'..=b'9') => Token::Number(Number::Integer),
            _ => return Err(()),
        };
        Ok(())
    }

    fn character(&mut self, purpose: Text, ch: char) -> Result<(), ()> {
        match purpose {
            Text::Ignored => {}
            Text::Key => {
                let mut encoded = [0; 4];
                self.key_hash.write(ch.encode_utf8(&mut encoded).as_bytes());
                // One character beyond the longest projected key distinguishes
                // a long unrelated name beginning with "invocation".
                if self.frames.len() == 1 && self.root_key.len() <= "invocation".len() {
                    self.root_key.push(ch);
                }
            }
            Text::Invocation => {
                let mut encoded = [0; 4];
                let part = ch.encode_utf8(&mut encoded);
                if !self.invocation.as_bytes()[self.invocation_bytes..].starts_with(part.as_bytes())
                {
                    return Err(());
                }
                self.invocation_bytes += part.len();
            }
            Text::Reply if !self.truncated => {
                if self.excerpt.len() + ch.len_utf8() <= EXCERPT_BYTES {
                    self.excerpt.push(ch);
                } else {
                    self.truncated = true;
                }
            }
            Text::Reply => {}
        }
        Ok(())
    }

    fn end_string(&mut self, purpose: Text) -> Result<(), ()> {
        if let Text::Key = purpose {
            let Some(Frame::Object(keys)) = self.frames.last_mut() else {
                return Err(());
            };
            // Hash decoded key characters, so an escaped spelling cannot mask a
            // duplicate. A collision conservatively withdraws only this preview.
            if self.open_keys == OPEN_OBJECT_KEYS || !keys.insert(self.key_hash.finish()) {
                return Err(());
            }
            self.open_keys += 1;
            if self.frames.len() == 1 {
                self.field = match self.root_key.as_str() {
                    "version" => Field::Version,
                    "invocation" => Field::Invocation,
                    "reply" => Field::Reply,
                    _ => Field::Other,
                };
            }
            self.token = Token::None;
            self.cursor = Cursor::Colon;
        } else {
            if let Text::Invocation = purpose {
                if self.invocation_bytes != self.invocation.len() {
                    return Err(());
                }
                self.invocation_seen = true;
            }
            self.end_value();
        }
        Ok(())
    }
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}
fn delimiter(byte: u8) -> bool {
    whitespace(byte) || matches!(byte, b',' | b'}' | b']')
}

#[derive(Clone, Copy)]
enum Number {
    Sign,
    Zero,
    Integer,
    Dot,
    Fraction,
    Exponent,
    ExponentSign,
    ExponentDigits,
}

impl Number {
    // False leaves the terminating separator for the structural parser.
    fn advance(&mut self, byte: u8) -> Result<bool, ()> {
        use Number::*;
        *self = match (*self, byte) {
            (Sign, b'0') => Zero,
            (Sign, b'1'..=b'9') | (Integer, b'0'..=b'9') => Integer,
            (Zero | Integer, b'.') => Dot,
            (Dot | Fraction, b'0'..=b'9') => Fraction,
            (Zero | Integer | Fraction, b'e' | b'E') => Exponent,
            (Exponent, b'-' | b'+') => ExponentSign,
            (Exponent | ExponentSign | ExponentDigits, b'0'..=b'9') => ExponentDigits,
            (Zero | Integer | Fraction | ExponentDigits, b) if delimiter(b) => return Ok(false),
            _ => return Err(()),
        };
        Ok(true)
    }
}

enum Decoded {
    More,
    Char(char),
    End,
}

#[derive(Clone, Copy)]
enum Decoder {
    Plain,
    Escape,
    Hex {
        value: u16,
        digits: u8,
        high: Option<u16>,
    },
    LowSlash(u16),
    LowU(u16),
    Utf8 {
        bytes: [u8; 4],
        len: usize,
    },
}

impl Decoder {
    fn push(&mut self, byte: u8) -> Result<Decoded, ()> {
        use Decoded::*;
        match *self {
            Self::Plain => match byte {
                b'"' => return Ok(End),
                b'\\' => *self = Self::Escape,
                0..=0x1f => return Err(()),
                0x20..=0x7f => return Ok(Char(char::from(byte))),
                0xc2..=0xf4 => {
                    *self = Self::Utf8 {
                        bytes: [byte, 0, 0, 0],
                        len: 1,
                    }
                }
                _ => return Err(()),
            },
            Self::Escape => {
                *self = Self::Plain;
                match byte {
                    b'"' | b'\\' | b'/' => return Ok(Char(char::from(byte))),
                    b'b' => return Ok(Char('\u{8}')),
                    b'f' => return Ok(Char('\u{c}')),
                    b'n' => return Ok(Char('\n')),
                    b'r' => return Ok(Char('\r')),
                    b't' => return Ok(Char('\t')),
                    b'u' => {
                        *self = Self::Hex {
                            value: 0,
                            digits: 0,
                            high: None,
                        }
                    }
                    _ => return Err(()),
                }
            }
            Self::Hex {
                value,
                digits,
                high,
            } => {
                let hex = char::from(byte).to_digit(16).ok_or(())? as u16;
                let value = (value << 4) | hex;
                if digits < 3 {
                    *self = Self::Hex {
                        value,
                        digits: digits + 1,
                        high,
                    };
                } else if let Some(high) = high {
                    if !(0xdc00..=0xdfff).contains(&value) {
                        return Err(());
                    }
                    let scalar =
                        0x10000 + (u32::from(high - 0xd800) << 10) + u32::from(value - 0xdc00);
                    *self = Self::Plain;
                    return Ok(Char(char::from_u32(scalar).ok_or(())?));
                } else if (0xd800..=0xdbff).contains(&value) {
                    *self = Self::LowSlash(value);
                } else {
                    *self = Self::Plain;
                    return Ok(Char(char::from_u32(u32::from(value)).ok_or(())?));
                }
            }
            Self::LowSlash(high) if byte == b'\\' => *self = Self::LowU(high),
            Self::LowU(high) if byte == b'u' => {
                *self = Self::Hex {
                    value: 0,
                    digits: 0,
                    high: Some(high),
                }
            }
            Self::LowSlash(_) | Self::LowU(_) => return Err(()),
            Self::Utf8 { mut bytes, len } => {
                bytes[len] = byte;
                match std::str::from_utf8(&bytes[..=len]) {
                    Ok(s) => {
                        *self = Self::Plain;
                        return Ok(Char(s.chars().next().ok_or(())?));
                    }
                    Err(error) if error.error_len().is_none() && len < 3 => {
                        *self = Self::Utf8 {
                            bytes,
                            len: len + 1,
                        };
                    }
                    Err(_) => return Err(()),
                }
            }
        }
        Ok(More)
    }
}
