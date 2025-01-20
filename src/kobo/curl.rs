use std::{
    borrow::Cow,
    io::{Cursor, Read, Seek, Write},
};

use curl::easy::{Easy2, Handler};

#[derive(Debug, Default)]
pub struct CurlAgent;

struct Collector<'a, W: Write> {
    inbody: Cursor<Cow<'a, [u8]>>,
    first: bool,
    headers: ::http::HeaderMap,
    body: W,
}

impl<'a, W: Write> Collector<'a, W> {
    pub fn new(inbody: Cow<'a, [u8]>, body: W) -> Self {
        Self {
            inbody: Cursor::new(inbody),
            headers: ::http::HeaderMap::new(),
            first: true,
            body,
        }
    }
}

impl<W: Write> Handler for Collector<'_, W> {
    fn header(&mut self, data: &[u8]) -> bool {
        if self.first {
            self.first = false;
            self.headers.clear();
            return true;
        }

        if data == b"\r\n" {
            self.first = true;
            return true;
        }

        let Some(i) = memchr::memchr(b':', data) else {
            return false;
        };
        let Ok(name) = ::http::HeaderName::from_bytes(&data[..i]) else {
            return false;
        };
        let value = &data[(i + 1)..];
        let value = if matches!(value.first().copied(), Some(b' ' | b'\t')) {
            &value[1..]
        } else {
            value
        };
        if !matches!(value.last(), Some(&b'\n')) {
            return false;
        }
        let value = &value[..value.len() - 1];
        if !matches!(value.last(), Some(&b'\r')) {
            return false;
        }
        let value = &value[..value.len() - 1];
        let Ok(value) = ::http::HeaderValue::from_bytes(value) else {
            return false;
        };
        self.headers.insert(name, value);
        true
    }

    fn write(&mut self, data: &[u8]) -> Result<usize, curl::easy::WriteError> {
        self.body.write_all(data).unwrap();
        Ok(data.len())
    }

    fn read(&mut self, data: &mut [u8]) -> Result<usize, curl::easy::ReadError> {
        self.inbody
            .read(data)
            .map_err(|_| curl::easy::ReadError::Abort)
    }

    fn seek(&mut self, whence: std::io::SeekFrom) -> curl::easy::SeekResult {
        match self.inbody.seek(whence) {
            Ok(_) => curl::easy::SeekResult::Ok,
            Err(_) => curl::easy::SeekResult::Fail,
        }
    }
}

fn header_name(name: &str, buf: &mut Vec<u8>) {
    #[inline(always)]
    fn push_char(buf: &mut Vec<u8>, c: char) {
        buf.extend_from_slice(c.encode_utf8(&mut [0u8; 4]).as_bytes());
    }

    let mut upcase = true;

    for c in name.chars() {
        match c {
            '-' => {
                push_char(buf, '-');
                upcase = true;
            }
            _ => {
                if upcase {
                    for c in c.to_uppercase() {
                        push_char(buf, c);
                    }
                    upcase = false;
                } else {
                    push_char(buf, c);
                }
            }
        }
    }
}

fn from_request<W: Write>(
    req: ::http::Request<super::Body>,
    outbody: W,
) -> Result<Easy2<Collector<W>>, ::curl::Error> {
    let (mut parts, body) = req.into_parts();

    let len = match &body {
        crate::Body::None => None,
        crate::Body::Data(cow) => Some(cow.len()),
    };

    let mut handle = Easy2::new(Collector::new(
        match body {
            crate::Body::None => Cow::Borrowed(b"".as_slice()),
            crate::Body::Data(cow) => cow,
        },
        outbody,
    ));

    match parts.version {
        ::http::Version::HTTP_09 => {
            handle.http_version(::curl::easy::HttpVersion::V10)?;
            handle.http_09_allowed(true)
        }
        ::http::Version::HTTP_10 => handle.http_version(::curl::easy::HttpVersion::V10),
        ::http::Version::HTTP_11 => handle.http_version(::curl::easy::HttpVersion::V11),
        ::http::Version::HTTP_2 => handle.http_version(::curl::easy::HttpVersion::V2),
        ::http::Version::HTTP_3 => handle.http_version(::curl::easy::HttpVersion::V3),
        _ => unreachable!(),
    }?;

    if let Some(len) = len {
        handle.upload(true)?;
        handle.in_filesize(len as u64)?;
    }

    handle.custom_request(parts.method.as_str())?;

    handle.url(&parts.uri.to_string())?;

    handle.accept_encoding("")?;
    let mut buf = Vec::new();
    let mut headers = ::curl::easy::List::new();
    for (name, value) in core::mem::take(&mut parts.headers) {
        let Some(name) = name else {
            continue;
        };
        buf.clear();
        buf.reserve_exact(name.as_str().len() + 2 + value.as_bytes().len());
        header_name(name.as_str(), &mut buf);
        buf.extend_from_slice(b": ");
        buf.extend_from_slice(value.as_bytes());
        headers.append(unsafe { std::str::from_utf8_unchecked(&buf) })?;
    }
    handle.http_headers(headers)?;
    handle.follow_location(false)?;

    Ok(handle)
}

struct WriteHolder<T>(Option<T>);

impl<T: Write> Write for WriteHolder<T> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.as_mut().unwrap().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.as_mut().unwrap().flush()
    }
}

impl super::Transport for CurlAgent {
    type Error = ::curl::Error;
    type Out = Cursor<Vec<u8>>;

    fn request<S: Send + Sync + 'static>(
        &mut self,
        req: ::http::Request<super::Body<'_>>,
    ) -> Result<http::Response<Self::Out>, super::Error<Self::Error, S>> {
        let mut handle = from_request(req, Vec::<u8>::new()).map_err(super::Error::Transport)?;

        handle.perform().map_err(super::Error::Transport)?;

        let mut parts = ::http::Response::new(()).into_parts().0;
        parts.headers = core::mem::take(&mut handle.get_mut().headers);
        parts.status = ::http::StatusCode::from_u16(
            handle.response_code().map_err(super::Error::Transport)? as u16,
        )
        .unwrap();

        Ok(::http::Response::from_parts(
            parts,
            Cursor::new(core::mem::take(&mut handle.get_mut().body)),
        ))
    }

    fn download<S: Send + Sync + 'static, W: Write>(
        &mut self,
        req: http::Request<super::Body<'_>>,
        output: W,
    ) -> Result<http::Response<W>, super::Error<Self::Error, S>> {
        let mut handle =
            from_request(req, WriteHolder(Some(output))).map_err(super::Error::Transport)?;

        handle.perform().map_err(super::Error::Transport)?;

        let mut parts = ::http::Response::new(()).into_parts().0;
        parts.headers = core::mem::take(&mut handle.get_mut().headers);
        parts.status = ::http::StatusCode::from_u16(
            handle.response_code().map_err(super::Error::Transport)? as u16,
        )
        .unwrap();
        let body = handle.get_mut().body.0.take().unwrap();

        Ok(::http::Response::from_parts(parts, body))
    }
}
