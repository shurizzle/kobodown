use std::borrow::Cow;

const TCHAR: [u8; 32] = [
    0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b11111010, 0b01101100, 0b11111111, 0b00000011,
    0b11111110, 0b11111111, 0b11111111, 0b11000111, 0b11111111, 0b11111111, 0b11111111, 0b01010111,
    0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000,
    0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000,
];

const QDTEXT: [u8; 32] = [
    0b00000000, 0b00000010, 0b00000000, 0b00000000, 0b11111011, 0b11111111, 0b11111111, 0b11111111,
    0b11111111, 0b11111111, 0b11111111, 0b11101111, 0b11111111, 0b11111111, 0b11111111, 0b01111111,
    0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b11111111,
    0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b11111111,
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MediaType<'a> {
    pub type_: Cow<'a, str>,
    pub subtype: Cow<'a, str>,
}

#[derive(Debug)]
pub struct InvalidContentType;

impl std::error::Error for InvalidContentType {}

pub struct ContentType<'a> {
    media: MediaType<'a>,
    rest: &'a [u8],
}

pub struct Iter<'a> {
    rest: &'a [u8],
}

pub struct Value<'a> {
    copy: bool,
    inner: &'a [u8],
}

fn ows(mut buf: &[u8]) -> &[u8] {
    while let Some((&c, b)) = buf.split_first() {
        if c != b' ' && c != b'\t' {
            break;
        }
        buf = b;
    }
    buf
}

#[inline(always)]
fn is_in(haystack: &[u8; 32], c: u8) -> bool {
    unsafe { (haystack.get_unchecked(c as usize / 8) & (1 << (c as usize % 8))) != 0 }
}

#[inline(always)]
fn is_token(c: u8) -> bool {
    is_in(&TCHAR, c)
}

#[inline(always)]
fn is_qdtext(c: u8) -> bool {
    is_in(&QDTEXT, c)
}

#[inline(always)]
const fn is_qchar(c: u8) -> bool {
    c == b'\t' || (c != 127 && c > 31)
}

fn skip_token(buf: &[u8]) -> Option<&[u8]> {
    let (&c, mut buf) = buf.split_first()?;
    if !is_token(c) {
        return None;
    }

    while let Some((&c, b)) = buf.split_first() {
        if !is_token(c) {
            break;
        }
        buf = b;
    }

    Some(buf)
}

fn pull_token(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    let tok = buf;
    let buf = skip_token(buf)?;
    let tok = unsafe { tok.get_unchecked(..(tok.len() - buf.len())) };
    Some((tok, buf))
}

fn pull_ident(buf: &[u8]) -> Option<(&str, &[u8])> {
    let (tok, buf) = pull_token(buf)?;
    Some((unsafe { std::str::from_utf8_unchecked(tok) }, buf))
}

fn pull_value(buf: &[u8]) -> Option<(Value<'_>, &[u8])> {
    let (v, buf) = match buf.split_first()? {
        (&b'"', mut buf) => {
            let start = buf;
            let mut copy = true;
            while let Some(c) = if let Some((&c, b)) = buf.split_first() {
                buf = b;
                Some(c)
            } else {
                None
            } {
                match c {
                    b'"' => {
                        return Some((
                            Value {
                                copy,
                                inner: unsafe {
                                    start.get_unchecked(..(start.len() - buf.len() - 1))
                                },
                            },
                            buf,
                        ))
                    }
                    b'\\' => {
                        let c;
                        (c, buf) = buf.split_first().map(|(&c, b)| (c, b))?;
                        if !is_qchar(c) {
                            return None;
                        }
                        copy = false;
                    }
                    _ => {
                        if !is_qdtext(c) {
                            return None;
                        }
                    }
                }
            }
            None
        }
        (_, _) => pull_token(buf).map(|(inner, b)| (Value { copy: true, inner }, b)),
    }?;

    if buf
        .first()
        .map(|&c| matches!(c, b' ' | b'\t' | b';'))
        .unwrap_or(true)
    {
        Some((v, buf))
    } else {
        None
    }
}

fn pull_parameter(buf: &[u8]) -> Option<(&str, Value<'_>, &[u8])> {
    let (key, buf) = pull_ident(buf)?;
    let (&c, buf) = buf.split_first()?;
    if c != b'=' {
        return None;
    }
    let (value, buf) = pull_value(buf)?;
    Some((key, value, buf))
}

impl MediaType<'_> {
    pub fn into_static(self) -> MediaType<'static> {
        MediaType {
            type_: Cow::Owned(self.type_.into_owned()),
            subtype: Cow::Owned(self.subtype.into_owned()),
        }
    }
}

impl core::fmt::Display for InvalidContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid Content-Type header")
    }
}

impl<'a> ContentType<'a> {
    pub fn parse(buf: &'a [u8]) -> Result<Self, InvalidContentType> {
        let buf = ows(buf);
        let (t, buf) = pull_ident(buf).ok_or(InvalidContentType)?;
        let (&c, buf) = buf.split_first().ok_or(InvalidContentType)?;
        if c != b'/' {
            return Err(InvalidContentType);
        }
        let (s, buf) = pull_ident(buf).ok_or(InvalidContentType)?;

        Ok(ContentType {
            media: MediaType {
                type_: Cow::Borrowed(t),
                subtype: Cow::Borrowed(s),
            },
            rest: buf,
        })
    }

    #[inline(always)]
    pub fn media_type(&self) -> &MediaType<'_> {
        &self.media
    }

    #[inline(always)]
    pub fn rest(&self) -> &[u8] {
        self.rest
    }

    #[inline(always)]
    pub fn parameters(&self) -> Iter<'a> {
        Iter { rest: self.rest }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = Result<(&'a str, Value<'a>), InvalidContentType>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        self.rest = ows(self.rest);
        {
            let (&c, b) = self.rest.split_first()?;
            self.rest = b;
            if c != b';' {
                self.rest = b"";
                return Some(Err(InvalidContentType));
            }
        };
        self.rest = ows(self.rest);
        if self.rest.is_empty() {
            return None;
        }
        if let Some((k, v, rest)) = pull_parameter(self.rest) {
            self.rest = rest;
            Some(Ok((k, v)))
        } else {
            self.rest = b"";
            Some(Err(InvalidContentType))
        }
    }
}

impl<'a> Value<'a> {
    pub fn value(&self) -> Cow<'a, [u8]> {
        if self.copy {
            Cow::Borrowed(self.inner)
        } else {
            let mut v = Vec::with_capacity(self.inner.len());
            self.value_in(&mut v);
            v.shrink_to_fit();
            Cow::Owned(v)
        }
    }

    pub fn value_in(&self, buf: &mut Vec<u8>) {
        if self.copy {
            buf.extend_from_slice(self.inner);
        } else {
            let mut it = self.inner.iter().copied();
            while let Some(mut c) = it.next() {
                if c == b'\\' {
                    c = it.next().unwrap();
                }
                buf.push(c);
            }
        }
    }
}
