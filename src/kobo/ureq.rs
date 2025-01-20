impl super::Transport for ::ureq::Agent {
    type Error = ::ureq::Error;
    type Out = ::ureq::BodyReader<'static>;

    fn request<S: Send + Sync + 'static>(
        &mut self,
        req: http::Request<super::Body<'_>>,
    ) -> Result<http::Response<Self::Out>, super::Error<Self::Error, S>> {
        let (mut parts, body) = req.into_parts();
        parts.extensions.insert(ureq_proto::CapitalizeHeaders);
        match body {
            super::Body::None => self.run(::http::Request::from_parts(parts, ())),
            super::Body::Data(cow) => self.run(::http::Request::from_parts(parts, cow.as_ref())),
        }
        .map(|res| {
            let (parts, body) = res.into_parts();
            ::http::Response::from_parts(parts, body.into_reader())
        })
        .map_err(super::Error::Transport)
    }

    fn download<S: Send + Sync + 'static, W: std::io::Write>(
        &mut self,
        req: http::Request<super::Body<'_>>,
        mut output: W,
    ) -> Result<http::Response<W>, super::Error<Self::Error, S>> {
        let (parts, mut body) = self.request(req)?.into_parts();
        std::io::copy(&mut body, &mut output)?;
        Ok(::http::Response::from_parts(parts, output))
    }
}
