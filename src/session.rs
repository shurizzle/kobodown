use super::NonEmptyStr;

pub trait Session {
    type Error: core::fmt::Debug + core::fmt::Display + Sync + Send + 'static;

    fn access_token(&self) -> Option<&NonEmptyStr>;

    fn device_id(&self) -> Option<&NonEmptyStr>;

    fn refresh_token(&self) -> Option<&NonEmptyStr>;

    fn user_id(&self) -> Option<&NonEmptyStr>;

    fn user_key(&self) -> Option<&NonEmptyStr>;

    fn remove_access_token(&mut self);

    fn remove_device_id(&mut self);

    fn remove_refresh_token(&mut self);

    fn remove_user_id(&mut self);

    fn remove_user_key(&mut self);

    fn set_access_token<S: Into<String>>(&mut self, v: Option<S>);

    fn set_device_id<S: Into<String>>(&mut self, v: Option<S>);

    fn set_refresh_token<S: Into<String>>(&mut self, v: Option<S>);

    fn set_user_id<S: Into<String>>(&mut self, v: Option<S>);

    fn set_user_key<S: Into<String>>(&mut self, v: Option<S>);

    fn save(&self) -> Result<(), Self::Error>;
}

impl<'a, S: Session + 'a> Session for &'a mut S {
    type Error = S::Error;

    fn access_token(&self) -> Option<&NonEmptyStr> {
        <S as Session>::access_token(self)
    }

    fn device_id(&self) -> Option<&NonEmptyStr> {
        <S as Session>::device_id(self)
    }

    fn refresh_token(&self) -> Option<&NonEmptyStr> {
        <S as Session>::refresh_token(self)
    }

    fn user_id(&self) -> Option<&NonEmptyStr> {
        <S as Session>::user_id(self)
    }

    fn user_key(&self) -> Option<&NonEmptyStr> {
        <S as Session>::user_key(self)
    }

    fn remove_access_token(&mut self) {
        <S as Session>::remove_access_token(self)
    }

    fn remove_device_id(&mut self) {
        <S as Session>::remove_device_id(self)
    }

    fn remove_refresh_token(&mut self) {
        <S as Session>::remove_refresh_token(self)
    }

    fn remove_user_id(&mut self) {
        <S as Session>::remove_user_id(self)
    }

    fn remove_user_key(&mut self) {
        <S as Session>::remove_user_key(self)
    }

    fn set_access_token<Str: Into<String>>(&mut self, v: Option<Str>) {
        <S as Session>::set_access_token(self, v)
    }

    fn set_device_id<Str: Into<String>>(&mut self, v: Option<Str>) {
        <S as Session>::set_device_id(self, v)
    }

    fn set_refresh_token<Str: Into<String>>(&mut self, v: Option<Str>) {
        <S as Session>::set_refresh_token(self, v)
    }

    fn set_user_id<Str: Into<String>>(&mut self, v: Option<Str>) {
        <S as Session>::set_user_id(self, v)
    }

    fn set_user_key<Str: Into<String>>(&mut self, v: Option<Str>) {
        <S as Session>::set_user_key(self, v)
    }

    fn save(&self) -> Result<(), Self::Error> {
        <S as Session>::save(self)
    }
}

pub struct SessionAdapter<S>(S);

impl<S> SessionAdapter<S> {
    pub fn new(session: S) -> Self {
        SessionAdapter(session)
    }
}

impl<S: Session> SessionAdapter<S> {
    pub fn is_auth_set(&self) -> bool {
        self.0.access_token().is_some()
            && self.0.device_id().is_some()
            && self.0.refresh_token().is_some()
    }

    pub fn is_logged_in(&self) -> bool {
        self.is_auth_set() && self.0.user_key().is_some() && self.0.user_id().is_some()
    }

    pub fn access_token(&self) -> Option<&NonEmptyStr> {
        if self.0.device_id().is_some() && self.0.refresh_token().is_some() {
            self.0.access_token()
        } else {
            None
        }
    }

    pub fn device_id(&self) -> Option<&NonEmptyStr> {
        self.0.device_id()
    }

    pub fn refresh_token(&self) -> Option<&NonEmptyStr> {
        if self.0.access_token().is_some() && self.0.device_id().is_some() {
            self.0.refresh_token()
        } else {
            None
        }
    }

    pub fn user_key(&self) -> Option<&NonEmptyStr> {
        if self.0.access_token().is_some()
            && self.0.device_id().is_some()
            && self.0.refresh_token().is_some()
            && self.0.user_id().is_some()
        {
            self.0.user_key()
        } else {
            None
        }
    }

    pub fn user_id(&self) -> Option<&NonEmptyStr> {
        if self.0.access_token().is_some()
            && self.0.device_id().is_some()
            && self.0.refresh_token().is_some()
            && self.0.user_key().is_some()
        {
            self.0.user_id()
        } else {
            None
        }
    }

    pub fn set_device_id<T: Into<String>>(&mut self, s: T) {
        self.0.set_device_id(Some(s));
        self.0.remove_access_token();
        self.0.remove_refresh_token();
        self.0.remove_user_key();
        self.0.remove_user_id();
    }

    pub fn set_user_key<T: Into<String>>(&mut self, s: T) {
        self.0.set_user_key(Some(s));
        self.0.remove_user_id();
    }

    pub fn set_user_id<T: Into<String>>(&mut self, s: T) {
        self.0.set_user_id(Some(s));
    }

    pub fn refresh_tokens<T1: Into<String>, T2: Into<String>>(&mut self, access: T1, refresh: T2) {
        let access = access.into();
        if access.is_empty() {
            return;
        }
        let refresh = refresh.into();
        if refresh.is_empty() {
            return;
        }
        self.0.set_access_token(Some(access));
        self.0.set_refresh_token(Some(refresh));
    }

    pub fn set_tokens<T1: Into<String>, T2: Into<String>>(&mut self, access: T1, refresh: T2) {
        let access = access.into();
        if access.is_empty() {
            return;
        }
        let refresh = refresh.into();
        if refresh.is_empty() {
            return;
        }
        self.0.set_access_token(Some(access));
        self.0.set_refresh_token(Some(refresh));
        self.0.remove_user_key();
        self.0.remove_user_id();
    }

    #[inline(always)]
    pub fn save(&self) -> Result<(), S::Error> {
        self.0.save()
    }

    #[inline(always)]
    pub fn inner(&self) -> &S {
        &self.0
    }

    #[inline(always)]
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.0
    }

    #[inline(always)]
    pub fn into_inner(self) -> S {
        self.0
    }
}
