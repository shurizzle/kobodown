mod config;
mod content_type;
mod kobo;
mod session;

use std::ops::Deref;

pub use config::*;
pub use content_type::*;
pub use kobo::*;
pub use session::*;

#[derive(Debug)]
pub struct NonEmptyStr(str);

impl Deref for NonEmptyStr {
    type Target = str;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::fmt::Display for NonEmptyStr {
    #[inline(always)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self)
    }
}

impl NonEmptyStr {
    pub fn new(s: &str) -> Option<&Self> {
        if s.is_empty() {
            None
        } else {
            Some(unsafe { Self::new_unchecked(s) })
        }
    }

    /// # Safety
    #[inline(always)]
    pub unsafe fn new_unchecked(s: &str) -> &Self {
        core::mem::transmute(s)
    }

    pub fn r#box(s: &str) -> Option<Box<Self>> {
        if s.is_empty() {
            None
        } else {
            Some(unsafe { Self::box_unchecked(s) })
        }
    }

    /// # Safety
    #[inline(always)]
    pub unsafe fn box_unchecked(s: &str) -> Box<Self> {
        Self::from_string_unchecked(s.to_string())
    }

    pub fn from_string(s: String) -> Option<Box<Self>> {
        if s.is_empty() {
            None
        } else {
            Some(unsafe { Self::from_string_unchecked(s) })
        }
    }

    /// # Safety
    #[inline(always)]
    pub unsafe fn from_string_unchecked(s: String) -> Box<Self> {
        core::mem::transmute(s.into_boxed_str())
    }

    pub fn from_box_str(s: Box<str>) -> Option<Box<Self>> {
        if s.is_empty() {
            None
        } else {
            Some(unsafe { Self::from_box_str_unchecked(s) })
        }
    }

    /// # Safety
    pub unsafe fn from_box_str_unchecked(s: Box<str>) -> Box<Self> {
        core::mem::transmute(s)
    }

    #[inline(always)]
    pub fn as_str(&self) -> &str {
        unsafe { core::mem::transmute(self) }
    }

    pub fn to_boxed_str(&self) -> Box<str> {
        self.to_string().into_boxed_str()
    }

    pub fn to_boxed_non_empty_str(&self) -> Box<NonEmptyStr> {
        unsafe { core::mem::transmute(self.to_boxed_str()) }
    }
}

pub struct EmptyStr(());

impl<'a> TryFrom<&'a str> for &'a NonEmptyStr {
    type Error = EmptyStr;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        NonEmptyStr::new(value).ok_or(EmptyStr(()))
    }
}

impl<'a> TryFrom<&'a Box<str>> for &'a NonEmptyStr {
    type Error = EmptyStr;

    fn try_from(value: &'a Box<str>) -> Result<Self, Self::Error> {
        NonEmptyStr::new(value).ok_or(EmptyStr(()))
    }
}

impl<'a> TryFrom<&'a String> for &'a NonEmptyStr {
    type Error = EmptyStr;

    fn try_from(value: &'a String) -> Result<Self, Self::Error> {
        NonEmptyStr::new(value).ok_or(EmptyStr(()))
    }
}

impl<'a> TryFrom<&'a str> for Box<NonEmptyStr> {
    type Error = EmptyStr;

    fn try_from(value: &'a str) -> Result<Self, Self::Error> {
        NonEmptyStr::new(value)
            .map(NonEmptyStr::to_boxed_non_empty_str)
            .ok_or(EmptyStr(()))
    }
}

impl<'a> TryFrom<&'a Box<str>> for Box<NonEmptyStr> {
    type Error = EmptyStr;

    fn try_from(value: &'a Box<str>) -> Result<Self, Self::Error> {
        NonEmptyStr::new(value)
            .map(NonEmptyStr::to_boxed_non_empty_str)
            .ok_or(EmptyStr(()))
    }
}

impl<'a> TryFrom<&'a String> for Box<NonEmptyStr> {
    type Error = EmptyStr;

    fn try_from(value: &'a String) -> Result<Self, Self::Error> {
        NonEmptyStr::new(value)
            .map(NonEmptyStr::to_boxed_non_empty_str)
            .ok_or(EmptyStr(()))
    }
}

impl TryFrom<String> for Box<NonEmptyStr> {
    type Error = EmptyStr;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        NonEmptyStr::from_string(value).ok_or(EmptyStr(()))
    }
}

impl TryFrom<Box<str>> for Box<NonEmptyStr> {
    type Error = EmptyStr;

    fn try_from(value: Box<str>) -> Result<Self, Self::Error> {
        NonEmptyStr::from_box_str(value).ok_or(EmptyStr(()))
    }
}

impl<'a> From<&'a NonEmptyStr> for &'a str {
    #[inline(always)]
    fn from(val: &'a NonEmptyStr) -> Self {
        val.as_str()
    }
}

impl<'a> From<&'a NonEmptyStr> for String {
    #[inline(always)]
    fn from(val: &'a NonEmptyStr) -> Self {
        val.to_string()
    }
}

impl From<Box<NonEmptyStr>> for Box<str> {
    #[inline(always)]
    fn from(val: Box<NonEmptyStr>) -> Self {
        unsafe { core::mem::transmute(val) }
    }
}

impl From<Box<NonEmptyStr>> for String {
    #[inline(always)]
    fn from(val: Box<NonEmptyStr>) -> Self {
        Box::<str>::from(val).into_string()
    }
}
