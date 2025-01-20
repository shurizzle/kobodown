use std::{borrow::Cow, io::Read};

use encoding_rs::Encoding;
use encoding_rs_io::DecodeReaderBytesBuilder;
use serde::{
    de::{DeserializeOwned, DeserializeSeed},
    Serialize,
};

use crate::ContentType;

use super::{Body, Error, Form, Json};

pub trait IntoRequest<'a> {
    fn into_request<E: std::error::Error + Send + Sync + 'static, S: Send + Sync + 'static>(
        self,
        parts: ::http::request::Parts,
    ) -> Result<::http::Request<Body<'a>>, Error<E, S>>;
}

pub trait FromResponse: Sized {
    fn from_response<
        E: std::error::Error + Send + Sync + 'static,
        S: Send + Sync + 'static,
        B: Read,
    >(
        parts: ::http::response::Parts,
        body: B,
    ) -> Result<Self, Error<E, S>>;
}

pub trait FromResponseSeed<'a> {
    type Value: Sized;

    #[allow(clippy::wrong_self_convention)]
    fn from_response<
        E: std::error::Error + Send + Sync + 'static,
        S: Send + Sync + 'static,
        B: Read,
    >(
        self,
        parts: ::http::response::Parts,
        body: B,
    ) -> Result<Self::Value, Error<E, S>>;
}

impl IntoRequest<'static> for () {
    fn into_request<E: std::error::Error + Send + Sync + 'static, S: Send + Sync + 'static>(
        self,
        parts: http::request::Parts,
    ) -> Result<http::Request<Body<'static>>, Error<E, S>> {
        Ok(http::Request::from_parts(parts, Body::None))
    }
}

impl FromResponse for () {
    fn from_response<
        E: std::error::Error + Send + Sync + 'static,
        S: Send + Sync + 'static,
        B: Read,
    >(
        parts: http::response::Parts,
        _body: B,
    ) -> Result<Self, Error<E, S>> {
        if parts.status.is_success() {
            Ok(())
        } else {
            Err(Error::StatusCode(parts.status))
        }
    }
}

impl<'a> IntoRequest<'a> for &'a [u8] {
    fn into_request<E: std::error::Error + Send + Sync + 'static, S: Send + Sync + 'static>(
        self,
        parts: http::request::Parts,
    ) -> Result<http::Request<Body<'a>>, Error<E, S>> {
        Ok(http::Request::from_parts(
            parts,
            Body::Data(Cow::Borrowed(self)),
        ))
    }
}

impl IntoRequest<'static> for Vec<u8> {
    fn into_request<E: std::error::Error + Send + Sync + 'static, S: Send + Sync + 'static>(
        self,
        parts: http::request::Parts,
    ) -> Result<http::Request<Body<'static>>, Error<E, S>> {
        Ok(http::Request::from_parts(
            parts,
            Body::Data(Cow::Owned(self)),
        ))
    }
}

fn content_length(res: &::http::response::Parts) -> Option<usize> {
    res.headers
        .get("Content-Length")
        .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
        .and_then(|l| l.parse().ok())
}

impl FromResponse for Vec<u8> {
    fn from_response<
        E: std::error::Error + Send + Sync + 'static,
        S: Send + Sync + 'static,
        B: Read,
    >(
        parts: http::response::Parts,
        mut body: B,
    ) -> Result<Self, Error<E, S>> {
        if parts.status == ::http::StatusCode::OK {
            let mut v = content_length(&parts)
                .map(Vec::with_capacity)
                .unwrap_or_default();
            body.read_to_end(&mut v)?;
            Ok(v)
        } else {
            Err(Error::StatusCode(parts.status))
        }
    }
}

fn charset_from_content_type<
    E: std::error::Error + Send + Sync + 'static,
    S: Send + Sync + 'static,
>(
    header: &[u8],
) -> Result<Option<Cow<[u8]>>, Error<E, S>> {
    let mut err = false;
    let mut res = None;
    for v in ContentType::parse(header)
        .map_err(|_| Error::InvalidContentType)?
        .parameters()
    {
        let (key, value) = v.map_err(|_| Error::InvalidContentType)?;
        if matches!(
            key.as_bytes(),
            [
                b'c' | b'C',
                b'h' | b'H',
                b'a' | b'A',
                b'r' | b'R',
                b's' | b'S',
                b'e' | b'E',
                b't' | b'T'
            ]
        ) {
            if res.is_some() {
                err = true;
            } else {
                res = Some(value.value());
            }
        }
    }
    if err {
        Err(Error::InvalidContentType)
    } else {
        Ok(res)
    }
}

fn is_utf8<T: AsRef<[u8]>>(s: T) -> bool {
    matches!(
        s.as_ref(),
        [b'U' | b'u', b'T' | b't', b'F' | b'f', b'8']
            | [b'U' | b'u', b'T' | b't', b'F' | b'f', b'-', b'8']
    )
}

impl<'a> IntoRequest<'a> for &'a str {
    fn into_request<E: std::error::Error + Send + Sync + 'static, S: Send + Sync + 'static>(
        self,
        parts: http::request::Parts,
    ) -> Result<http::Request<Body<'a>>, Error<E, S>> {
        if let Some(ct) = parts
            .headers
            .get("Content-Type")
            .map(http::HeaderValue::as_bytes)
            .map(charset_from_content_type)
            .unwrap_or(Ok(None))?
        {
            if !is_utf8(ct.as_ref()) {
                return Encoding::for_label(ct.as_ref())
                    .ok_or_else(|| Error::InvalidEncoding(ct.to_vec().into_boxed_slice()))
                    .map(|enc| {
                        let (me, _, _) = enc.encode(self);
                        http::Request::from_parts(parts, Body::Data(me))
                    });
            }
        }

        Ok(http::Request::from_parts(
            parts,
            Body::Data(Cow::Borrowed(self.as_bytes())),
        ))
    }
}

impl IntoRequest<'static> for Box<str> {
    fn into_request<E: std::error::Error + Send + Sync + 'static, S: Send + Sync + 'static>(
        self,
        parts: http::request::Parts,
    ) -> Result<http::Request<Body<'static>>, Error<E, S>> {
        IntoRequest::into_request(self.into_string(), parts)
    }
}

impl FromResponse for Box<str> {
    fn from_response<
        E: std::error::Error + Send + Sync + 'static,
        S: Send + Sync + 'static,
        B: Read,
    >(
        parts: http::response::Parts,
        body: B,
    ) -> Result<Self, Error<E, S>> {
        <String as FromResponse>::from_response(parts, body).map(String::into_boxed_str)
    }
}

impl IntoRequest<'static> for String {
    fn into_request<E: std::error::Error + Send + Sync + 'static, S: Send + Sync + 'static>(
        self,
        parts: http::request::Parts,
    ) -> Result<http::Request<Body<'static>>, Error<E, S>> {
        if let Some(ct) = parts
            .headers
            .get("Content-Type")
            .map(http::HeaderValue::as_bytes)
            .map(charset_from_content_type)
            .unwrap_or(Ok(None))?
        {
            if !is_utf8(ct.as_ref()) {
                return Encoding::for_label(ct.as_ref())
                    .ok_or_else(|| Error::InvalidEncoding(ct.to_vec().into_boxed_slice()))
                    .map(|enc| {
                        let (me, _, _) = enc.encode(&self);
                        http::Request::from_parts(
                            parts,
                            Body::Data(Cow::Owned(match me {
                                Cow::Borrowed(_) => self.into_bytes(),
                                Cow::Owned(v) => v,
                            })),
                        )
                    });
            }
        }
        Ok(http::Request::from_parts(
            parts,
            Body::Data(Cow::Owned(self.into_bytes())),
        ))
    }
}

impl FromResponse for String {
    fn from_response<
        E: std::error::Error + Send + Sync + 'static,
        S: Send + Sync + 'static,
        B: Read,
    >(
        parts: http::response::Parts,
        mut body: B,
    ) -> Result<Self, Error<E, S>> {
        if parts.status != ::http::StatusCode::OK {
            return Err(Error::StatusCode(parts.status));
        }
        if let Some(ct) = parts
            .headers
            .get("Content-Type")
            .map(http::HeaderValue::as_bytes)
            .map(charset_from_content_type)
            .unwrap_or(Ok(None))?
        {
            if !is_utf8(ct.as_ref()) {
                let mut buf = content_length(&parts)
                    .map(String::with_capacity)
                    .unwrap_or_default();
                DecodeReaderBytesBuilder::new()
                    .encoding(Some(Encoding::for_label(ct.as_ref()).ok_or_else(|| {
                        Error::InvalidEncoding(ct.to_vec().into_boxed_slice())
                    })?))
                    .build(body)
                    .read_to_string(&mut buf)
                    .map_err(|_| Error::InvalidEncoding(ct.to_vec().into_boxed_slice()))?;
                return Ok(buf);
            }
        }
        let mut buf = content_length(&parts)
            .map(String::with_capacity)
            .unwrap_or_default();
        body.read_to_string(&mut buf)
            .map_err(|_| Error::InvalidEncoding(b"UTF-8".to_vec().into_boxed_slice()))?;
        Ok(buf)
    }
}

impl<T: Serialize> IntoRequest<'static> for Json<T> {
    fn into_request<E: std::error::Error + Send + Sync + 'static, S: Send + Sync + 'static>(
        self,
        mut parts: http::request::Parts,
    ) -> Result<http::Request<Body<'static>>, Error<E, S>> {
        if let Some(ct) = parts
            .headers
            .get("Content-Type")
            .map(http::HeaderValue::as_bytes)
        {
            if !charset_from_content_type(ct)?.map(is_utf8).unwrap_or(true) {
                return Encoding::for_label(ct)
                    .ok_or_else(|| Error::InvalidEncoding(ct.to_vec().into_boxed_slice()))
                    .and_then(|enc| {
                        let b = serde_json::to_string(&self.0).map_err(Error::Json)?;
                        let (me, _, _) = enc.encode(&b);
                        Ok(http::Request::from_parts(
                            parts,
                            Body::Data(Cow::Owned(match me {
                                Cow::Borrowed(_) => b.into_bytes(),
                                Cow::Owned(v) => v,
                            })),
                        ))
                    });
            }
        } else {
            parts.headers.insert(
                "Content-Type",
                http::HeaderValue::from_static("application/json; charset=utf-8"),
            );
        }
        serde_json::to_vec(&self.0)
            .map(|b| http::Request::from_parts(parts, Body::Data(Cow::Owned(b))))
            .map_err(Error::Json)
    }
}

impl<T: Serialize> IntoRequest<'static> for Form<T> {
    fn into_request<E: std::error::Error + Send + Sync + 'static, S: Send + Sync + 'static>(
        self,
        mut parts: http::request::Parts,
    ) -> Result<http::Request<Body<'static>>, Error<E, S>> {
        if let Some(ct) = parts
            .headers
            .get("Content-Type")
            .map(http::HeaderValue::as_bytes)
        {
            if !charset_from_content_type(ct)?.map(is_utf8).unwrap_or(true) {
                return Encoding::for_label(ct)
                    .ok_or_else(|| Error::InvalidEncoding(ct.to_vec().into_boxed_slice()))
                    .and_then(|enc| {
                        let b = serde_qs::to_string(&self.0).map_err(Error::Form)?;
                        let (me, _, _) = enc.encode(&b);
                        Ok(http::Request::from_parts(
                            parts,
                            Body::Data(Cow::Owned(match me {
                                Cow::Borrowed(_) => b.into_bytes(),
                                Cow::Owned(v) => v,
                            })),
                        ))
                    });
            }
        } else {
            parts.headers.insert(
                "Content-Type",
                http::HeaderValue::from_static("application/x-www-form-urlencoded; charset=utf-8"),
            );
        }
        serde_qs::to_string(&self.0)
            .map(|b| http::Request::from_parts(parts, Body::Data(Cow::Owned(b.into_bytes()))))
            .map_err(Error::Form)
    }
}

impl<T: DeserializeOwned> FromResponse for Json<T> {
    fn from_response<
        E: std::error::Error + Send + Sync + 'static,
        S: Send + Sync + 'static,
        B: Read,
    >(
        parts: ::http::response::Parts,
        body: B,
    ) -> Result<Self, Error<E, S>> {
        if parts.status != ::http::StatusCode::OK {
            return Err(Error::StatusCode(parts.status));
        }
        if let Some(ct) = parts
            .headers
            .get("Content-Type")
            .map(http::HeaderValue::as_bytes)
            .map(charset_from_content_type)
            .unwrap_or(Ok(None))?
        {
            if !is_utf8(ct.as_ref()) {
                return serde_json::from_reader(
                    DecodeReaderBytesBuilder::new()
                        .encoding(Some(Encoding::for_label(ct.as_ref()).ok_or_else(|| {
                            Error::InvalidEncoding(ct.to_vec().into_boxed_slice())
                        })?))
                        .build(body),
                )
                .map(Json)
                .map_err(Error::Json);
            }
        }
        serde_json::from_reader(body).map(Json).map_err(Error::Json)
    }
}

impl<'a, T: DeserializeSeed<'a>> FromResponseSeed<'a> for Json<T> {
    type Value = T::Value;

    fn from_response<
        E: std::error::Error + Send + Sync + 'static,
        S: Send + Sync + 'static,
        B: Read,
    >(
        self,
        parts: ::http::response::Parts,
        body: B,
    ) -> Result<Self::Value, Error<E, S>> {
        if parts.status != ::http::StatusCode::OK {
            return Err(Error::StatusCode(parts.status));
        }
        if let Some(ct) = parts
            .headers
            .get("Content-Type")
            .map(http::HeaderValue::as_bytes)
            .map(charset_from_content_type)
            .unwrap_or(Ok(None))?
        {
            if !is_utf8(ct.as_ref()) {
                let mut body = serde_json::Deserializer::from_reader(
                    DecodeReaderBytesBuilder::new()
                        .encoding(Some(Encoding::for_label(ct.as_ref()).ok_or_else(|| {
                            Error::InvalidEncoding(ct.to_vec().into_boxed_slice())
                        })?))
                        .build(body),
                );
                let res = self.0.deserialize(&mut body).map_err(Error::Json)?;
                return body.end().map(|_| res).map_err(Error::Json);
            }
        }
        let mut body = serde_json::Deserializer::from_reader(body);
        let res = self.0.deserialize(&mut body).map_err(Error::Json)?;
        body.end().map(|_| res).map_err(Error::Json)
    }
}
