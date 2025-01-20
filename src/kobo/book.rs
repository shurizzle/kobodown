use std::{collections::HashMap, marker::PhantomData};

use base64::Engine;
use serde::{
    de::{DeserializeSeed, Unexpected, Visitor},
    Deserialize,
};
use url::Url;

use super::FromResponse;

struct MatchStringVisitor<F: Fn(&str) -> bool>(F);

impl<'de, F: Fn(&str) -> bool> Visitor<'de> for MatchStringVisitor<F> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string")
    }

    fn visit_char<E>(self, v: char) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_str(v.encode_utf8(&mut [0u8; 4]))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if (self.0)(v) {
            Ok(())
        } else {
            Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(v),
                &self,
            ))
        }
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_str(v)
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_str(&v)
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if let Ok(v) = std::str::from_utf8(v) {
            self.visit_str(v)
        } else {
            Err(serde::de::Error::invalid_type(
                serde::de::Unexpected::Bytes(v),
                &self,
            ))
        }
    }

    fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_bytes(v)
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_bytes(&v)
    }
}

#[derive(Debug, Default)]
struct False;

impl<'de> Deserialize<'de> for False {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct FalseVisitor;
        impl Visitor<'_> for FalseVisitor {
            type Value = False;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("false")
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v {
                    Err(serde::de::Error::invalid_type(
                        serde::de::Unexpected::Bool(v),
                        &self,
                    ))
                } else {
                    Ok(False)
                }
            }
        }

        deserializer.deserialize_bool(FalseVisitor)
    }
}

#[derive(Debug)]
pub struct Book {
    pub authors: Option<Box<str>>,
    pub title: Box<str>,
    pub revision_id: Box<str>,
    pub is_archived: bool,
}

impl core::fmt::Display for Book {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.title)?;
        if let Some(ref authors) = self.authors {
            write!(f, " by {authors}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct StatusInfoStatus;

impl<'de> Deserialize<'de> for StatusInfoStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_string(MatchStringVisitor(|s| s != "Finished"))?;
        Ok(Self)
    }
}

#[derive(Debug, Deserialize)]
pub struct StatusInfo {
    #[serde(default, rename = "Status")]
    _status: StatusInfoStatus,
}

#[derive(Debug, Deserialize)]
pub struct ReadingState {
    #[serde(rename = "StatusInfo")]
    _status_info: StatusInfo,
}

#[derive(Debug, Default)]
pub struct Accessibility;

impl<'de> Deserialize<'de> for Accessibility {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_string(MatchStringVisitor(|s| s != "Preview"))?;
        Ok(Self)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BookEntitlement {
    #[serde(default)]
    _accessibility: Accessibility,
    #[serde(default)]
    _is_locked: False,
    pub is_removed: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContributorRole {
    role: Option<Box<str>>,
    name: Box<str>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BookMetadata {
    pub revision_id: Box<str>,
    pub title: Box<str>,
    pub contributor_roles: Option<Vec<ContributorRole>>,
}

fn authors(mut contributor_roles: Vec<ContributorRole>) -> Option<Box<str>> {
    let mut authors = String::new();
    for name in vec_extract_if_polyfill::MakeExtractIf::extract_if(&mut contributor_roles, |r| {
        r.role
            .as_ref()
            .map(|s| s.as_ref() == "Author")
            .unwrap_or(false)
    })
    .map(|r| r.name)
    {
        if !authors.is_empty() {
            authors.push_str(" & ");
        }
        authors.push_str(&name);
    }
    if authors.is_empty() {
        if let Some(author) = contributor_roles.first_mut() {
            authors = std::mem::take(&mut author.name).into_string();
        }
    }
    if authors.is_empty() {
        None
    } else {
        Some(authors.into_boxed_str())
    }
}

impl From<BookMetadata> for Book {
    fn from(
        BookMetadata {
            revision_id,
            title,
            contributor_roles,
        }: BookMetadata,
    ) -> Self {
        Book {
            authors: contributor_roles.and_then(authors),
            title,
            revision_id,
            is_archived: false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NewEntitlement {
    pub book_entitlement: Option<BookEntitlement>,
    pub book_metadata: BookMetadata,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NewEntitlementFull {
    pub book_entitlement: Option<BookEntitlement>,
    #[serde(rename = "ReadingState")]
    _reading_state: ReadingState,
    pub book_metadata: BookMetadata,
}

pub trait Entitlement: for<'de> Deserialize<'de> {
    fn book_entitlement(&mut self) -> Option<&mut BookEntitlement>;

    fn book_metadata(&mut self) -> &mut BookMetadata;

    fn to_book(mut self) -> Book {
        let is_archived = self
            .book_entitlement()
            .and_then(|e| e.is_removed)
            .unwrap_or(false);
        let mut res: Book = std::mem::take(self.book_metadata()).into();
        res.is_archived = is_archived;
        res
    }
}

impl Entitlement for NewEntitlement {
    fn book_entitlement(&mut self) -> Option<&mut BookEntitlement> {
        self.book_entitlement.as_mut()
    }

    fn book_metadata(&mut self) -> &mut BookMetadata {
        &mut self.book_metadata
    }
}

impl From<NewEntitlement> for Book {
    fn from(value: NewEntitlement) -> Self {
        value.to_book()
    }
}

impl Entitlement for NewEntitlementFull {
    fn book_entitlement(&mut self) -> Option<&mut BookEntitlement> {
        self.book_entitlement.as_mut()
    }

    fn book_metadata(&mut self) -> &mut BookMetadata {
        &mut self.book_metadata
    }
}

impl From<NewEntitlementFull> for Book {
    fn from(value: NewEntitlementFull) -> Self {
        value.to_book()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct KoboBook<E> {
    pub new_entitlement: E,
}

impl<T: Entitlement> From<KoboBook<T>> for Book {
    fn from(KoboBook { new_entitlement }: KoboBook<T>) -> Self {
        new_entitlement.to_book()
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Maybe<E> {
    Some(E),
    None(serde::de::IgnoredAny),
}

impl<T> From<Maybe<T>> for Option<T> {
    fn from(value: Maybe<T>) -> Self {
        match value {
            Maybe::Some(v) => Some(v),
            Maybe::None(_) => None,
        }
    }
}

#[derive(Debug)]
pub struct NoneOnError<T>(pub Option<T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for NoneOnError<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self(
            <Maybe<T> as Deserialize<'de>>::deserialize(deserializer)?.into(),
        ))
    }
}

#[derive(Debug)]
pub struct Books<T>(pub Vec<Book>, PhantomData<T>);
struct BooksVisitor<T>(PhantomData<T>);

impl<'de, T: Entitlement> Visitor<'de> for BooksVisitor<T> {
    type Value = Books<T>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("an array of books")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut vec = Vec::new();
        while let Some(book) = seq.next_element()? {
            let NoneOnError::<KoboBook<T>>(Some(book)) = book else {
                continue;
            };
            vec.push(book.into());
        }
        Ok(Books(vec, PhantomData))
    }
}

impl<'de, T: Entitlement> Deserialize<'de> for Books<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(BooksVisitor::<T>(PhantomData))
    }
}

pub struct BooksPage<T: Entitlement> {
    pub books: Vec<Book>,
    pub next: Option<Box<str>>,
    _entitlement: PhantomData<T>,
}

impl<T: Entitlement> FromResponse for BooksPage<T> {
    fn from_response<
        E: std::error::Error + Send + Sync + 'static,
        S: Send + Sync + 'static,
        B: std::io::Read,
    >(
        parts: http::response::Parts,
        body: B,
    ) -> Result<Self, super::Error<E, S>> {
        if parts.status != ::http::StatusCode::OK {
            return Err(super::Error::StatusCode(parts.status));
        }
        let next = if parts
            .headers
            .get("x-kobo-sync")
            .map(|h| h.as_bytes() == b"continue")
            .unwrap_or(false)
        {
            parts
                .headers
                .get("x-kobo-synctoken")
                .and_then(|h| {
                    if h.as_bytes().is_empty() {
                        None
                    } else {
                        std::str::from_utf8(h.as_bytes()).ok()
                    }
                })
                .map(|s| s.to_string().into_boxed_str())
        } else {
            None
        };

        Ok(BooksPage {
            books: <super::Json<Books<T>> as FromResponse>::from_response(parts, body)?
                .0
                 .0,
            next,
            _entitlement: PhantomData,
        })
    }
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::upper_case_acronyms)]
enum DRMType {
    KDRM,
    SignedNoDrm,
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum UrlFormat {
    EPUB3,
    EPUB3FL,
    KEPUB,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawContentUrl {
    #[serde(rename = "DRMType")]
    drm_type: DRMType,
    #[serde(rename = "UrlFormat")]
    _url_format: UrlFormat,
    #[serde(with = "super::url")]
    download_url: Url,
    byte_size: u64,
}

#[derive(Debug)]
struct ContentUrl {
    pub has_drm: bool,
    pub url: Url,
    pub size: u64,
}

impl<'de> Deserialize<'de> for ContentUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ContentUrlVisitor;
        impl<'de> Visitor<'de> for ContentUrlVisitor {
            type Value = ContentUrl;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a download url")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                while let Some(c) = seq.next_element::<NoneOnError<RawContentUrl>>()? {
                    let Some(mut c) = c.0 else {
                        continue;
                    };
                    if let Some(q) = c.download_url.query() {
                        let mut qs = String::new();
                        let mut f = false;
                        for kv in q.split("&").filter(|s| !s.is_empty()) {
                            if kv.starts_with("b=")
                                || kv.starts_with("%62=")
                                || kv == "b"
                                || kv == "%62="
                            {
                                f = true;
                            } else {
                                if !qs.is_empty() {
                                    qs.push('&');
                                }
                                qs.push_str(kv);
                            }
                        }
                        let qs = if qs.is_empty() { None } else { Some(qs) };
                        if f {
                            c.download_url.set_query(qs.as_deref());
                        }
                    }
                    return Ok(ContentUrl {
                        has_drm: c.drm_type == DRMType::KDRM,
                        url: c.download_url,
                        size: c.byte_size,
                    });
                }
                Err(serde::de::Error::invalid_value(
                    Unexpected::Seq,
                    &"a download url",
                ))
            }
        }

        deserializer.deserialize_seq(ContentUrlVisitor)
    }
}

#[derive(Debug)]
struct ContentKeyValue(::aes::cipher::Key<aes::Aes128Dec>);
#[derive(Debug)]
struct ContentKeyValueDeserializer<'a>(&'a ::aes::cipher::Key<aes::Aes128Dec>);

impl<'de> DeserializeSeed<'de> for ContentKeyValueDeserializer<'_> {
    type Value = ContentKeyValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Debug)]
        struct VisitorImpl<'a>(&'a ::aes::cipher::Key<aes::Aes128Dec>);

        impl<'de> Visitor<'de> for VisitorImpl<'_> {
            type Value = ContentKeyValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("base64 AES128 key")
            }

            fn visit_char<E>(self, v: char) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(v.encode_utf8(&mut [0u8; 4]))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                use aes::cipher::{BlockDecryptMut, KeyInit};
                type Aes128EcbDec = ecb::Decryptor<aes::Aes128>;

                base64::engine::general_purpose::STANDARD
                    .decode(v)
                    .ok()
                    .and_then(|v| TryInto::<[u8; 16]>::try_into(v).ok())
                    .and_then(|mut v| {
                        if Aes128EcbDec::new(self.0)
                            .decrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut v)
                            .ok()?
                            .len()
                            == 16
                        {
                            Some(ContentKeyValue(v.into()))
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| serde::de::Error::invalid_value(Unexpected::Str(v), &self))
            }

            fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(v)
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_str(&v)
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if let Ok(v) = std::str::from_utf8(v) {
                    self.visit_str(v)
                } else {
                    Err(serde::de::Error::invalid_type(Unexpected::Bytes(v), &self))
                }
            }

            fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_bytes(v)
            }

            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_bytes(&v)
            }
        }

        deserializer.deserialize_string(VisitorImpl(self.0))
    }
}

#[derive(Debug)]
struct ContentKey {
    pub name: Box<str>,
    pub value: ::aes::cipher::Key<aes::Aes128Dec>,
}

#[derive(Debug)]
struct ContentKeyDeserializer<'a>(&'a ::aes::cipher::Key<aes::Aes128Dec>);

impl<'de> DeserializeSeed<'de> for ContentKeyDeserializer<'_> {
    type Value = ContentKey;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        enum Field {
            Name,
            Value,
            Ignore,
        }
        struct FieldVisitor;
        impl Visitor<'_> for FieldVisitor {
            type Value = Field;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("field indentifier")
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    0 => Ok(Field::Name),
                    1 => Ok(Field::Value),
                    _ => Ok(Field::Ignore),
                }
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    "Name" => Ok(Field::Name),
                    "Value" => Ok(Field::Value),
                    _ => Ok(Field::Ignore),
                }
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    b"Name" => Ok(Field::Name),
                    b"Value" => Ok(Field::Value),
                    _ => Ok(Field::Ignore),
                }
            }
        }
        impl<'de> Deserialize<'de> for Field {
            #[inline]
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct VisitorImpl<'a>(&'a ::aes::cipher::Key<aes::Aes128Dec>);
        impl<'de> Visitor<'de> for VisitorImpl<'_> {
            type Value = ContentKey;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct ContentKey")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let Some(name) = seq.next_element::<Box<str>>()? else {
                    return Err(serde::de::Error::invalid_length(
                        0,
                        &"struct ContentKey with 2 elements",
                    ));
                };
                let Some(value) = seq
                    .next_element_seed(ContentKeyValueDeserializer(self.0))?
                    .map(|v| v.0)
                else {
                    return Err(serde::de::Error::invalid_length(
                        1,
                        &"struct ContentKey with 2 elements",
                    ));
                };
                Ok(ContentKey { name, value })
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut name = None;
                let mut value = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Name => {
                            if name.is_some() {
                                return Err(serde::de::Error::duplicate_field("Name"));
                            }
                            name = Some(map.next_value()?);
                        }
                        Field::Value => {
                            if value.is_some() {
                                return Err(serde::de::Error::duplicate_field("Value"));
                            }
                            value =
                                Some(map.next_value_seed(ContentKeyValueDeserializer(self.0))?.0);
                        }
                        Field::Ignore => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }

                let Some(name) = name else {
                    return Err(serde::de::Error::missing_field("Name"));
                };
                let Some(value) = value else {
                    return Err(serde::de::Error::missing_field("Value"));
                };
                Ok(ContentKey { name, value })
            }
        }

        const FIELDS: &[&str] = &["Name", "Value"];
        deserializer.deserialize_struct("ContentKey", FIELDS, VisitorImpl(self.0))
    }
}

#[derive(Debug, Default)]
struct ContentKeys2(HashMap<Box<str>, ::aes::cipher::Key<aes::Aes128Dec>>);

struct ContentKeysDeserializer<'a>(&'a ::aes::cipher::Key<aes::Aes128Dec>);
impl<'de> DeserializeSeed<'de> for ContentKeysDeserializer<'_> {
    type Value = ContentKeys2;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ContentKeysVisitor<'a>(&'a ::aes::cipher::Key<aes::Aes128Dec>);
        impl<'de> Visitor<'de> for ContentKeysVisitor<'_> {
            type Value = ContentKeys2;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("content keys")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut res = HashMap::new();
                while let Some(e) = seq.next_element_seed(ContentKeyDeserializer(self.0))? {
                    res.insert(e.name, e.value);
                }
                Ok(ContentKeys2(res))
            }
        }
        deserializer.deserialize_seq(ContentKeysVisitor(self.0))
    }
}

#[derive(Debug)]
pub struct AccessBook {
    pub url: Url,
    pub size: u64,
    pub content_keys: Option<HashMap<Box<str>, ::aes::cipher::Key<aes::Aes128Dec>>>,
}

pub struct AccessBookDeserializer<'a>(pub &'a ::aes::cipher::Key<aes::Aes128Dec>);

impl<'de> DeserializeSeed<'de> for AccessBookDeserializer<'_> {
    type Value = AccessBook;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        enum Field {
            ContentUrls,
            ContentKeys,
            Ignore,
        }
        struct FieldVisitor;
        impl Visitor<'_> for FieldVisitor {
            type Value = Field;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("field indentifier")
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    0 => Ok(Field::ContentUrls),
                    1 => Ok(Field::ContentKeys),
                    _ => Ok(Field::Ignore),
                }
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    "ContentUrls" => Ok(Field::ContentUrls),
                    "ContentKeys" => Ok(Field::ContentKeys),
                    _ => Ok(Field::Ignore),
                }
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match v {
                    b"ContentUrls" => Ok(Field::ContentUrls),
                    b"ContentKeys" => Ok(Field::ContentKeys),
                    _ => Ok(Field::Ignore),
                }
            }
        }
        impl<'de> Deserialize<'de> for Field {
            #[inline]
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct VisitorImpl<'a>(&'a ::aes::cipher::Key<aes::Aes128Dec>);
        impl<'de> Visitor<'de> for VisitorImpl<'_> {
            type Value = AccessBook;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("struct AccessBook")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let Some(ContentUrl { has_drm, url, size }) = seq.next_element::<ContentUrl>()?
                else {
                    return Err(serde::de::Error::invalid_length(
                        0,
                        &"struct AccessBook with 2 elements",
                    ));
                };
                let content_keys = if has_drm {
                    let Some(content_keys) = seq
                        .next_element_seed(ContentKeysDeserializer(self.0))?
                        .map(|v| v.0)
                    else {
                        return Err(serde::de::Error::invalid_length(
                            1,
                            &"struct AccessBook with 2 elements",
                        ));
                    };
                    Some(content_keys)
                } else {
                    None
                };

                Ok(AccessBook {
                    url,
                    size,
                    content_keys,
                })
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut content_url = None;
                let mut content_keys = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::ContentUrls => {
                            if content_url.is_some() {
                                return Err(serde::de::Error::duplicate_field("ContentUrls"));
                            }
                            content_url = Some(map.next_value()?);
                        }
                        Field::ContentKeys => {
                            if content_keys.is_some() {
                                return Err(serde::de::Error::duplicate_field("ContentKeys"));
                            }
                            content_keys =
                                Some(map.next_value_seed(ContentKeysDeserializer(self.0))?.0);
                        }
                        Field::Ignore => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }

                let Some(ContentUrl { has_drm, url, size }) = content_url else {
                    return Err(serde::de::Error::missing_field("ContentUrls"));
                };
                let content_keys = if has_drm {
                    let Some(content_keys) = content_keys else {
                        return Err(serde::de::Error::missing_field("ContentKeys"));
                    };
                    Some(content_keys)
                } else {
                    None
                };

                Ok(AccessBook {
                    url,
                    size,
                    content_keys,
                })
            }
        }

        const FIELDS: &[&str] = &["ContentUrls", "ContentKeys"];
        deserializer.deserialize_struct("AccessBook", FIELDS, VisitorImpl(self.0))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawBookInfo {
    pub title: Box<str>,
    pub contributor_roles: Option<Vec<ContributorRole>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(from = "RawBookInfo")]
pub struct BookInfo {
    pub author: Option<Box<str>>,
    pub title: Box<str>,
}

impl From<RawBookInfo> for BookInfo {
    fn from(
        RawBookInfo {
            title,
            contributor_roles,
        }: RawBookInfo,
    ) -> Self {
        BookInfo {
            author: contributor_roles.and_then(authors),
            title,
        }
    }
}
