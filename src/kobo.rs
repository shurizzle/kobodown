mod book;
#[cfg(feature = "curl")]
mod curl;
mod js;
mod request;
#[cfg(feature = "ureq")]
mod ureq;
mod url;

pub use book::{AccessBook, Book, BookInfo};
#[cfg(feature = "curl")]
pub use curl::CurlAgent;
pub use request::*;

use scraper::{Html, Selector};

use std::{borrow::Cow, io::Read, str::FromStr, sync::LazyLock};

use ::url::Url;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::instrument;

use crate::{Session, SessionAdapter};

const AFFILIATE: &str = "Kobo";
const APPLICATION_VERSION: &str = "10.1.2.39807";
const DEFAULT_PLATFORM_ID: &str = "00000000-0000-0000-0000-000000004000";
const CARRIER_NAME: &str = "310270";
const DEVICE_MODEL: &str = "Pixel";
const DEVICE_OS_VERSION: &str = "33";
const DISPLAY_PROFILE: &str = "Android";
// Use the user agent of the Kobo Android app, otherwise the login request hangs forever.
const USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 13; Pixel Build/TQ2B.230505.005.A1; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/101.0.4951.61 Safari/537.36 KoboApp/10.1.2.39807 KoboPlatform Id/00000000-0000-0000-0000-000000004000 KoboAffiliate/Kobo KoboBuildFlavor/global";

cfg_if::cfg_if! {
    if #[cfg(feature = "curl")] {
        pub type DefaultAgent = CurlAgent;
    } else if #[cfg(feature = "ureq")] {
        pub type DefaultAgent = ::ureq::Agent;
    } else {
        compiler_error!("No transport available.");
    }
}

fn default_headers<T>(req: &mut http::Request<T>) {
    let hs = req.headers_mut();
    hs.insert("User-Agent", http::HeaderValue::from_static(USER_AGENT));
    hs.insert(
        "x-kobo-affiliatename",
        http::HeaderValue::from_static(AFFILIATE),
    );
    hs.insert(
        "x-kobo-appversion",
        http::HeaderValue::from_static(APPLICATION_VERSION),
    );
    hs.insert(
        "x-kobo-platformid",
        http::HeaderValue::from_static(DEFAULT_PLATFORM_ID),
    );
    hs.insert(
        "x-kobo-carriername",
        http::HeaderValue::from_static(CARRIER_NAME),
    );
    hs.insert(
        "x-kobo-devicemodel",
        http::HeaderValue::from_static(DEVICE_MODEL),
    );
    hs.insert("x-kobo-deviceos", http::HeaderValue::from_static("Android"));
    hs.insert(
        "x-kobo-deviceosversion",
        http::HeaderValue::from_static(DEVICE_OS_VERSION),
    );
    hs.insert(
        "X-Requested-With",
        http::HeaderValue::from_static("com.kobobooks.android"),
    );
    hs.insert(
        "Accept-Encoding",
        http::HeaderValue::from_static("gzip, deflate"),
    );
}

#[derive(Debug)]
pub struct Json<T>(pub T);

impl<T> Json<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for Json<T> {
    fn from(value: T) -> Self {
        Json(value)
    }
}

#[derive(Debug)]
pub struct Form<T>(pub T);

#[derive(thiserror::Error, Debug)]
pub enum Error<T: std::error::Error + Send + Sync + 'static, S: Send + Sync + 'static> {
    #[error("Invalid encoding {0:?}")]
    InvalidEncoding(Box<[u8]>),
    #[error("Invalid Content-Type")]
    InvalidContentType,
    #[error("Not logged in")]
    NotLoggedIn,
    #[error("Invalid login flow")]
    LoginFlow,
    #[error("Invalid status code {0}")]
    StatusCode(::http::StatusCode),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(serde_json::Error),
    #[error("{0}")]
    Form(serde_qs::Error),
    #[error("{0}")]
    Transport(T),
    #[error("{0}")]
    Session(S),
}

pub enum Body<'a> {
    None,
    Data(Cow<'a, [u8]>),
    // TODO: Read interface?
}

pub trait Transport {
    type Error: std::error::Error + Send + Sync + 'static;
    type Out: Read;

    fn request<S: Send + Sync + 'static>(
        &mut self,
        req: http::Request<Body<'_>>,
    ) -> Result<http::Response<Self::Out>, Error<Self::Error, S>>;

    fn download<S: Send + Sync + 'static, W: std::io::Write>(
        &mut self,
        req: http::Request<Body<'_>>,
        output: W,
    ) -> Result<::http::Response<W>, Error<Self::Error, S>>;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(with = "url")]
    sign_in_page: Url,
    book: Box<str>,
    #[serde(with = "url")]
    library_sync: Url,
    #[serde(with = "url")]
    user_wishlist: Url,
    content_access_book: Box<str>,
}

#[derive(Debug)]
pub struct Kobo<T: Transport> {
    settings: Option<Settings>,
    cookies: cookie_store::CookieStore,
    transport: T,
}

fn mkreq(method: ::http::Method, uri: ::http::Uri) -> ::http::request::Parts {
    let (mut parts, ()) = ::http::Request::new(()).into_parts();
    parts.method = method;
    parts.uri = uri;
    parts.version = ::http::Version::HTTP_11;
    parts
}

fn is_cookie_rfc_compliant(cookie: &cookie_store::Cookie) -> bool {
    #[inline]
    pub(crate) fn is_tchar(b: &u8) -> bool {
        match b {
            b'!' | b'#' | b'$' | b'%' | b'&' => true,
            b'\'' | b'*' | b'+' | b'-' | b'.' => true,
            b'^' | b'_' | b'`' | b'|' | b'~' => true,
            b if b.is_ascii_alphanumeric() => true,
            _ => false,
        }
    }

    fn is_valid_name(b: &u8) -> bool {
        is_tchar(b)
    }

    fn is_valid_value(b: &u8) -> bool {
        b.is_ascii()
            && !b.is_ascii_control()
            && !b.is_ascii_whitespace()
            && *b != b'"'
            && *b != b','
            && *b != b';'
            && *b != b'\\'
    }

    let name = cookie.name().as_bytes();

    let valid_name = name.iter().all(is_valid_name);

    if !valid_name {
        return false;
    }

    let value = cookie.value().as_bytes();

    let valid_value = value.iter().all(is_valid_value);

    if !valid_value {
        return false;
    }

    true
}

impl<T: Transport> Kobo<T> {
    pub fn new(transport: T) -> Self {
        Self {
            settings: None,
            cookies: ::cookie_store::CookieStore::new(None),
            transport,
        }
    }

    fn push_cookies<B>(&self, url: &::url::Url, req: &mut ::http::Request<B>) {
        let mut cookies = String::new();
        for cookie in self.cookies.matches(url) {
            if !is_cookie_rfc_compliant(cookie) {
                continue;
            }
            if !cookies.is_empty() {
                cookies.push(';');
            }

            use core::fmt::Write;
            _ = write!(cookies, "{}", cookie.stripped());
        }

        req.headers_mut().insert(
            ::http::header::COOKIE,
            ::http::HeaderValue::from_maybe_shared(::bytes::Bytes::from(cookies.into_bytes()))
                .unwrap(),
        );
    }

    fn pull_cookies<B>(&mut self, url: &::url::Url, res: &::http::Response<B>) {
        self.cookies.store_response_cookies(
            res.headers()
                .get_all(::http::header::SET_COOKIE)
                .iter()
                .filter_map(|h| h.to_str().ok())
                .filter_map(|v| ::cookie_store::Cookie::parse(v, url).ok())
                .map(::cookie_store::Cookie::into_owned)
                .map(Into::into),
            url,
        );
    }

    fn raw_request<'a, InB: IntoRequest<'a>, S: Send + Sync + 'static>(
        &mut self,
        mut req: http::Request<InB>,
    ) -> Result<::http::Response<T::Out>, Error<T::Error, S>> {
        default_headers(&mut req);
        let (parts, body) = req.into_parts();
        let url = ::url::Url::parse(&parts.uri.to_string()).unwrap();
        let mut r = body.into_request(parts)?;
        self.push_cookies(&url, &mut r);
        let res = self.transport.request(r)?;
        self.pull_cookies(&url, &res);
        if !res.status().is_redirection() {
            return Ok(res);
        }
        let mut url = if let Some(u) = res
            .headers()
            .get_all(::http::header::LOCATION)
            .iter()
            .filter_map(|h| h.to_str().ok())
            .filter_map(|u| url.join(u).ok())
            .next()
        {
            u
        } else {
            return Ok(res);
        };
        let mut req = ::http::Request::from_parts(
            mkreq(
                ::http::Method::GET,
                ::http::Uri::from_str(url.as_str()).unwrap(),
            ),
            (),
        );
        loop {
            default_headers(&mut req);
            let (parts, body) = req.into_parts();
            let mut r = body.into_request(parts)?;
            self.push_cookies(&url, &mut r);
            let res = self.transport.request(r)?;
            self.pull_cookies(&url, &res);
            if !res.status().is_redirection() {
                return Ok(res);
            }
            url = if let Some(u) = res
                .headers()
                .get_all(::http::header::LOCATION)
                .iter()
                .filter_map(|h| h.to_str().ok())
                .filter_map(|u| url.join(u).ok())
                .next()
            {
                u
            } else {
                return Ok(res);
            };
            req = ::http::Request::from_parts(
                mkreq(
                    ::http::Method::GET,
                    ::http::Uri::from_str(url.as_str()).unwrap(),
                ),
                (),
            );
        }
    }

    fn simple_request<'a, InB: IntoRequest<'a>, OutB: FromResponse, S: Send + Sync + 'static>(
        &mut self,
        req: http::Request<InB>,
    ) -> Result<OutB, Error<T::Error, S>> {
        let (parts, body) = self.raw_request(req)?.into_parts();
        OutB::from_response(parts, body)
    }

    #[allow(clippy::type_complexity)]
    #[inline(always)]
    fn _anon_raw_request<'a, InB, S, F>(
        &mut self,
        session: &mut SessionAdapter<S>,
        req: http::Request<F>,
    ) -> Result<::http::Response<T::Out>, Error<T::Error, S::Error>>
    where
        InB: IntoRequest<'a>,
        S: Session,
        F: Fn() -> InB,
    {
        let (mut parts, body) = req.into_parts();

        let res = {
            let auth = self.get_authorization(session)?;
            let mut parts = parts.clone();
            parts.headers.insert("Authorization", auth);
            self.raw_request(::http::Request::from_parts(parts, body()))?
        };

        if res.status() != ::http::StatusCode::UNAUTHORIZED {
            return Ok(res);
        }
        parts
            .headers
            .insert("Authorization", self.refresh_auth(session)?);
        self.raw_request(::http::Request::from_parts(parts, body()))
    }

    fn anon_request<'a, InB, OutB, S, F>(
        &mut self,
        session: &mut SessionAdapter<S>,
        req: http::Request<F>,
    ) -> Result<OutB, Error<T::Error, S::Error>>
    where
        InB: IntoRequest<'a>,
        OutB: FromResponse,
        S: Session,
        F: Fn() -> InB,
    {
        let (parts, body) = self._anon_raw_request(session, req)?.into_parts();
        OutB::from_response(parts, body)
    }

    #[allow(clippy::type_complexity)]
    #[inline(always)]
    fn _raw_request<'a, InB, S, F>(
        &mut self,
        session: &mut SessionAdapter<S>,
        req: http::Request<F>,
    ) -> Result<::http::Response<T::Out>, Error<T::Error, S::Error>>
    where
        InB: IntoRequest<'a>,
        S: Session,
        F: Fn() -> InB,
    {
        if !session.is_logged_in() {
            return Err(Error::NotLoggedIn);
        }

        let (mut parts, body) = req.into_parts();

        let res = {
            if let Some(auth) = session
                .access_token()
                .and_then(|s| ::http::HeaderValue::from_str(&format!("Bearer {s}")).ok())
            {
                let mut parts = parts.clone();
                parts.headers.insert("Authorization", auth);
                self.raw_request(::http::Request::from_parts(parts, body()))?
            } else {
                return Err(Error::NotLoggedIn);
            }
        };

        if res.status() != ::http::StatusCode::UNAUTHORIZED {
            return Ok(res);
        }

        parts
            .headers
            .insert("Authorization", self.refresh_auth(session)?);
        if !session.is_logged_in() {
            return Err(Error::NotLoggedIn);
        }
        self.raw_request(::http::Request::from_parts(parts, body()))
    }

    fn request<'a, InB, OutB, S, F>(
        &mut self,
        session: &mut SessionAdapter<S>,
        req: http::Request<F>,
    ) -> Result<OutB, Error<T::Error, S::Error>>
    where
        InB: IntoRequest<'a>,
        OutB: FromResponse,
        S: Session,
        F: Fn() -> InB,
    {
        let (parts, body) = self._raw_request(session, req)?.into_parts();
        OutB::from_response(parts, body)
    }

    fn request_seed<'a, InB, Seed, S, F>(
        &mut self,
        session: &mut SessionAdapter<S>,
        req: http::Request<F>,
        seed: Seed,
    ) -> Result<Seed::Value, Error<T::Error, S::Error>>
    where
        InB: IntoRequest<'a>,
        Seed: FromResponseSeed<'a>,
        S: Session,
        F: Fn() -> InB,
    {
        let (parts, body) = self._raw_request(session, req)?.into_parts();
        seed.from_response(parts, body)
    }

    #[instrument(skip(self, session))]
    fn refresh_auth<S: Session>(
        &mut self,
        session: &mut SessionAdapter<S>,
    ) -> Result<::http::HeaderValue, Error<T::Error, S::Error>> {
        #[derive(Debug, Serialize)]
        #[serde(rename_all = "PascalCase")]
        struct RequestBody<'a> {
            app_version: &'static str,
            client_key: Box<str>,
            platform_id: &'static str,
            refresh_token: &'a str,
        }

        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct ResponseBody {
            token_type: String,
            access_token: String,
            refresh_token: String,
        }

        if let (Some(access_token), Some(refresh_token)) = (
            session
                .access_token()
                .and_then(|s| ::http::HeaderValue::from_str(&format!("Bearer {s}")).ok()),
            session.refresh_token(),
        ) {
            let mut parts = mkreq(
                ::http::Method::POST,
                ::http::Uri::from_static("https://storeapi.kobo.com/v1/auth/refresh"),
            );
            parts.headers.insert("Authorization", access_token);
            let ResponseBody {
                token_type,
                access_token,
                refresh_token,
            } = self
                .simple_request::<_, Json<ResponseBody>, _>(::http::Request::from_parts(
                    parts,
                    Json(RequestBody {
                        app_version: APPLICATION_VERSION,
                        client_key: base64::prelude::BASE64_STANDARD
                            .encode(DEFAULT_PLATFORM_ID.as_bytes())
                            .into_boxed_str(),
                        platform_id: DEFAULT_PLATFORM_ID,
                        refresh_token,
                    }),
                ))?
                .into_inner();
            assert!(token_type == "Bearer");
            session.refresh_tokens(access_token, refresh_token);
        }

        self.get_authorization(session)
    }

    fn get_authorization<S: Session>(
        &mut self,
        session: &mut SessionAdapter<S>,
    ) -> Result<::http::HeaderValue, Error<T::Error, S::Error>> {
        loop {
            if let Some(access_token) = session.access_token() {
                if let Ok(h) = ::http::HeaderValue::from_str(&format!("Bearer {access_token}")) {
                    return Ok(h);
                }
                session.set_tokens("", "");
            }
            self.authenticate_device(session, None)?;
            session.save().map_err(Error::Session)?;
        }
    }

    #[instrument(skip(self, session))]
    fn authenticate_device<S: Session>(
        &mut self,
        session: &mut SessionAdapter<S>,
        user_key: Option<String>,
    ) -> Result<(), Error<T::Error, S::Error>> {
        #[derive(Serialize)]
        #[serde(rename_all = "PascalCase")]
        struct RequestBody<'a> {
            affiliate_name: &'a str,
            app_version: &'a str,
            client_key: Box<str>,
            device_id: &'a str,
            platform_id: &'a str,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "PascalCase")]
        struct RequestBodyFull<'a> {
            affiliate_name: &'a str,
            app_version: &'a str,
            client_key: Box<str>,
            device_id: &'a str,
            platform_id: &'a str,
            user_key: &'a str,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct ResponseBody {
            token_type: Box<str>,
            access_token: Box<str>,
            refresh_token: Box<str>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct ResponseBodyFull {
            token_type: Box<str>,
            access_token: Box<str>,
            refresh_token: Box<str>,
            user_key: Box<str>,
        }

        let user_key = user_key.and_then(|s| if s.is_empty() { None } else { Some(s) });

        if session.is_auth_set() && user_key.is_none() {
            return Ok(());
        }

        let device_id = loop {
            if let Some(id) = session.device_id() {
                break id;
            }
            session.set_device_id(uuid::Uuid::now_v7().to_string());
        };
        let parts = mkreq(
            ::http::Method::POST,
            ::http::Uri::from_static("https://storeapi.kobo.com/v1/auth/device"),
        );

        let (res, user_key) = if let Some(user_key) = user_key.as_deref() {
            let Json(ResponseBodyFull {
                token_type,
                access_token,
                refresh_token,
                user_key,
            }) = self.simple_request(::http::Request::from_parts(
                parts,
                Json(RequestBodyFull {
                    affiliate_name: AFFILIATE,
                    app_version: APPLICATION_VERSION,
                    client_key: base64::prelude::BASE64_STANDARD
                        .encode(DEFAULT_PLATFORM_ID.as_bytes())
                        .into_boxed_str(),
                    device_id: device_id.as_str(),
                    platform_id: DEFAULT_PLATFORM_ID,
                    user_key,
                }),
            ))?;
            (
                ResponseBody {
                    token_type,
                    access_token,
                    refresh_token,
                },
                Some(user_key),
            )
        } else {
            (
                self.simple_request::<_, Json<_>, _>(::http::Request::from_parts(
                    parts,
                    Json(RequestBody {
                        affiliate_name: AFFILIATE,
                        app_version: APPLICATION_VERSION,
                        client_key: base64::prelude::BASE64_STANDARD
                            .encode(DEFAULT_PLATFORM_ID.as_bytes())
                            .into_boxed_str(),
                        device_id: device_id.as_str(),
                        platform_id: DEFAULT_PLATFORM_ID,
                    }),
                ))?
                .into_inner(),
                None,
            )
        };

        assert!(&*res.token_type == "Bearer");
        session.set_tokens(res.access_token, res.refresh_token);
        if let Some(user_key) = user_key {
            session.set_user_key(user_key);
        }
        Ok(())
    }

    #[instrument(skip(self, session))]
    fn settings<S: Session>(
        &mut self,
        session: &mut SessionAdapter<S>,
    ) -> Result<&Settings, Error<T::Error, S::Error>> {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Inner {
            pub resources: Settings,
        }

        loop {
            if let Some(ref res) = self.settings {
                return Ok(res);
            }

            let parts = mkreq(
                ::http::Method::GET,
                ::http::Uri::from_static("https://storeapi.kobo.com/v1/initialization"),
            );
            self.settings = Some(
                self.anon_request::<_, Json<Inner>, _, _>(
                    session,
                    ::http::Request::from_parts(parts, || ()),
                )?
                .into_inner()
                .resources,
            );
        }
    }

    #[allow(clippy::type_complexity)]
    fn login_parameters<S: Session>(
        &mut self,
        session: &mut SessionAdapter<S>,
    ) -> Result<(String, String, Url), Error<T::Error, S::Error>> {
        fn extract(doc: &Html) -> Option<(String, String)> {
            static FORM_SELECTOR: LazyLock<Selector> = LazyLock::new(|| {
                Selector::parse("section#defaultOptions form:has(#signInBlock)").unwrap()
            });
            static WORKFLOW_ID_SELECTOR: LazyLock<Selector> =
                LazyLock::new(|| Selector::parse("input[name=\"LogInModel.WorkflowId\"]").unwrap());
            static TOKEN_SELECTOR: LazyLock<Selector> = LazyLock::new(|| {
                Selector::parse("input[name=\"__RequestVerificationToken\"]").unwrap()
            });

            let form = doc.select(&FORM_SELECTOR).next()?;
            let workflow_id = form.select(&WORKFLOW_ID_SELECTOR).next()?.attr("value")?;
            if workflow_id.is_empty() {
                return None;
            }
            let token = form.select(&TOKEN_SELECTOR).next()?.attr("value")?;
            if token.is_empty() {
                return None;
            }
            Some((workflow_id.to_string(), token.to_string()))
        }

        let mut url = self.settings(session)?.sign_in_page.clone();
        url.query_pairs_mut()
            .append_pair("wsa", AFFILIATE)
            .append_pair("pwsav", APPLICATION_VERSION)
            .append_pair("pwspid", DEFAULT_PLATFORM_ID)
            .append_pair("pwsdid", session.device_id().unwrap())
            .append_pair("wscfv", "1.5")
            .append_pair("wscf", "kepub")
            .append_pair("wsmc", CARRIER_NAME)
            .append_pair("pwspov", DEVICE_OS_VERSION)
            .append_pair("pwspt", "Mobile")
            .append_pair("pwsdm", DEVICE_MODEL);
        let parts = mkreq(
            ::http::Method::GET,
            ::http::Uri::from_str(url.as_str()).unwrap(),
        );
        let page = self.simple_request::<_, String, _>(::http::Request::from_parts(parts, ()))?;
        let (workflow_id, token) = extract(&Html::parse_document(&page)).ok_or(Error::LoginFlow)?;
        let mut url = self.settings(session)?.sign_in_page.clone();
        url.set_query(None);
        url.set_path("/ww/en/signin/signin");
        Ok((workflow_id, token, url))
    }

    #[instrument(
        skip(self, session, username, password, captcha),
        fields(username = username, password = "***", captcha = captcha)
    )]
    pub fn login<S: Session>(
        &mut self,
        session: S,
        username: &str,
        password: &str,
        captcha: &str,
    ) -> Result<(), Error<T::Error, S::Error>> {
        static SCRIPT_SELECTOR: LazyLock<Selector> =
            LazyLock::new(|| Selector::parse("script").unwrap());

        #[derive(Debug, Serialize)]
        struct RequestBody<'a> {
            #[serde(rename = "LogInModel.WorkflowId")]
            workflow_id: String,
            #[serde(rename = "LogInModel.Provider")]
            provider: &'static str,
            #[serde(rename = "ReturnUrl")]
            return_url: &'static str,
            #[serde(rename = "__RequestVerificationToken")]
            token: String,
            #[serde(rename = "LogInModel.UserName")]
            username: &'a str,
            #[serde(rename = "LogInModel.Password")]
            password: &'a str,
            #[serde(rename = "g-recaptcha-response")]
            g_captcha: &'a str,
            #[serde(rename = "h-captcha-response")]
            h_captcha: &'a str,
        }

        let mut session = SessionAdapter::new(session);
        let (workflow_id, token, url) = self.login_parameters(&mut session)?;

        let parts = mkreq(
            ::http::Method::POST,
            ::http::Uri::from_str(url.as_str()).unwrap(),
        );
        let body = RequestBody {
            workflow_id,
            provider: AFFILIATE,
            return_url: "",
            token,
            username,
            password,
            g_captcha: captcha,
            h_captcha: captcha,
        };
        let page = self.anon_request::<_, String, _, _>(
            &mut session,
            ::http::Request::from_parts(parts, || Form(&body)),
        )?;
        let doc = Html::parse_document(&page);
        let mut script = "var location={};\n".to_string();
        for s in doc.select(&SCRIPT_SELECTOR) {
            script.push_str("try{\n");
            for txt in s.text() {
                script.push_str(txt);
                script.push('\n');
            }
            script.push_str("}catch(____e){}\n");
        }
        let url = Url::parse(&js::extract_href(script).ok_or(Error::LoginFlow)?)
            .map_err(|_| Error::LoginFlow)?;
        let mut user_id = None;
        let mut user_key = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "userId" => user_id = Some(value.into_owned()),
                "userKey" => user_key = Some(value.into_owned()),
                _ => (),
            }
        }
        let (user_id, user_key) = match (user_id, user_key) {
            (Some(i), Some(k)) => (i, k),
            _ => return Err(Error::LoginFlow),
        };

        self.authenticate_device(&mut session, Some(user_key))?;
        session.set_user_id(user_id);
        session.save().map_err(Error::Session)?;
        Ok(())
    }

    fn sync_page<E: book::Entitlement, S: Session>(
        &mut self,
        session: &mut SessionAdapter<S>,
        token: Option<&str>,
    ) -> Result<book::BooksPage<E>, Error<T::Error, S::Error>> {
        let token = token
            .and_then(|s| if s.is_empty() { None } else { Some(s) })
            .and_then(|s| ::http::HeaderValue::from_str(s).ok());
        let mut parts = mkreq(
            ::http::Method::GET,
            ::http::Uri::from_str(self.settings(session)?.library_sync.as_str()).unwrap(),
        );
        if let Some(token) = token {
            parts.headers.insert("x-kobo-synctoken", token);
        }
        self.request::<_, book::BooksPage<E>, _, _>(
            session,
            ::http::Request::from_parts(parts, || ()),
        )
    }

    fn _book_list<E: book::Entitlement, S: Session>(
        &mut self,
        session: &mut SessionAdapter<S>,
    ) -> Result<Vec<Book>, Error<T::Error, S::Error>> {
        let mut token = None;
        let mut res = Vec::new();
        loop {
            let book::BooksPage { books, next, .. } =
                self.sync_page::<E, S>(session, token.as_deref())?;
            res.extend(books);
            token = next;
            if token.is_none() {
                res.sort_by(|a, b| a.title.as_ref().cmp(b.title.as_ref()));
                return Ok(res);
            }
        }
    }

    #[instrument(skip(self, session))]
    pub fn book_list<S: Session>(
        &mut self,
        session: S,
        all: bool,
    ) -> Result<Vec<Book>, Error<T::Error, S::Error>> {
        if all {
            self._book_list::<book::NewEntitlement, _>(&mut SessionAdapter::new(session))
        } else {
            self._book_list::<book::NewEntitlementFull, _>(&mut SessionAdapter::new(session))
        }
    }

    pub fn access_book<S: Session>(
        &mut self,
        session: S,
        product_id: &str,
    ) -> Result<AccessBook, Error<T::Error, S::Error>> {
        let mut session = SessionAdapter::new(session);
        let url = {
            let mut url = String::new();
            for (i, p) in self
                .settings(&mut session)?
                .content_access_book
                .split("{ProductId}")
                .enumerate()
            {
                if i != 0 {
                    url.push_str(product_id);
                }
                url.push_str(p);
            }
            let mut url = Url::parse(&url).unwrap();
            url.query_pairs_mut()
                .append_pair("DisplayProfile", DISPLAY_PROFILE);
            ::http::Uri::from_str(url.as_str()).unwrap()
        };
        let key = if let (Some(device_id), Some(user_id)) = (session.device_id(), session.user_id())
        {
            use std::io::Write;

            let mut sha = Sha256::new();
            write!(&mut sha, "{device_id}{user_id}").unwrap();
            let a: [u8; 32] = sha.finalize().into();
            Into::<aes::cipher::Key<aes::Aes128Dec>>::into([
                a[16], a[17], a[18], a[19], a[20], a[21], a[22], a[23], a[24], a[25], a[26], a[27],
                a[28], a[29], a[30], a[31],
            ])
        } else {
            return Err(Error::NotLoggedIn);
        };
        let parts = mkreq(::http::Method::GET, url);
        self.request_seed(
            &mut session,
            ::http::Request::from_parts(parts, || ()),
            Json(book::AccessBookDeserializer(&key)),
        )
    }

    #[instrument(skip(self, session))]
    pub fn book_info<S: Session>(
        &mut self,
        session: S,
        product_id: &str,
    ) -> Result<BookInfo, Error<T::Error, S::Error>> {
        let mut session = SessionAdapter::new(session);
        let parts = mkreq(::http::Method::GET, {
            let mut url = String::new();
            for (i, p) in self
                .settings(&mut session)?
                .book
                .split("{ProductId}")
                .enumerate()
            {
                if i != 0 {
                    url.push_str(product_id);
                }
                url.push_str(p);
            }
            ::http::Uri::from_str(url.as_str()).unwrap()
        });
        self.request::<_, Json<BookInfo>, _, _>(
            &mut session,
            ::http::Request::from_parts(parts, || ()),
        )
        .map(Json::into_inner)
    }

    #[instrument(skip(self, session, output))]
    pub fn download<S: Session, W: std::io::Write>(
        &mut self,
        session: S,
        url: &::url::Url,
        output: W,
    ) -> Result<W, Error<T::Error, S::Error>> {
        let session = SessionAdapter::new(session);
        if !session.is_logged_in() {
            return Err(Error::NotLoggedIn);
        }

        let mut parts = mkreq(
            ::http::Method::GET,
            ::http::Uri::from_str(url.as_str()).unwrap(),
        );

        if let Some(auth) = session
            .access_token()
            .and_then(|s| ::http::HeaderValue::from_str(&format!("Bearer {s}")).ok())
        {
            parts.headers.insert("Authorization", auth);
        } else {
            return Err(Error::NotLoggedIn);
        }
        let mut req = ::http::Request::from_parts(parts, Body::None);
        default_headers(&mut req);
        let (parts, body) = {
            self.push_cookies(url, &mut req);
            let res = self.transport.download(req, output)?;
            self.pull_cookies(url, &res);
            res.into_parts()
        };

        if !parts.status.is_success() {
            return Err(Error::StatusCode(parts.status));
        }
        Ok(body)
    }
}

impl Default for Kobo<DefaultAgent> {
    fn default() -> Self {
        cfg_if::cfg_if! {
            if #[cfg(feature = "curl")] {
                Kobo::new(CurlAgent)
            } else if #[cfg(feature = "ureq")] {
                Kobo::new(::ureq::config::Config::builder().http_status_as_error(false).build().new_agent())
            } else {
                compiler_error!("No transport available.");
            }
        }
    }
}
