use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
    sync::LazyLock,
};

use crate::Session;

use super::NonEmptyStr;
use color_eyre::eyre::{Context, Report};
use directories::ProjectDirs;
use tracing::instrument;

#[derive(Default, Debug)]
pub struct Config(serde_json::Value);

static CFG_FILE: LazyLock<PathBuf> = LazyLock::new(|| {
    let dirs = ProjectDirs::from("dev", "shurizzle", "kobodown").unwrap();
    dirs.config_dir().join("kobodown.json")
});

impl Config {
    fn gets<'a>(&'a self, name: &str) -> Option<&'a NonEmptyStr> {
        if let serde_json::Value::Object(ref obj) = self.0 {
            if let serde_json::Value::String(ref obj) = obj.get(name)? {
                NonEmptyStr::new(obj)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn dels(&mut self, name: &str) {
        let serde_json::Value::Object(ref mut obj) = self.0 else {
            return;
        };
        obj.remove(name);
    }

    fn set(&mut self, name: &str, v: serde_json::Value) {
        let obj = loop {
            if let serde_json::Value::Object(ref mut obj) = self.0 {
                break obj;
            } else {
                self.0 = serde_json::Value::Object(Default::default());
            }
        };

        if let Some(vv) = obj.get_mut(name) {
            *vv = v;
            return;
        }
        obj.insert(name.to_string(), v);
    }

    fn sets<S: Into<String>>(&mut self, name: &str, value: Option<S>) {
        if let Some(value) = value.map(Into::into).and_then(NonEmptyStr::from_string) {
            self.set(name, serde_json::Value::String(value.to_string()))
        } else {
            self.dels(name)
        }
    }

    #[instrument]
    pub fn load() -> Self {
        Self(
            File::open(&*CFG_FILE)
                .ok()
                .and_then(|f| serde_json::from_reader(BufReader::new(f)).ok())
                .unwrap_or_default(),
        )
    }
}

impl Session for Config {
    type Error = Report;

    fn access_token(&self) -> Option<&NonEmptyStr> {
        self.gets("AccessToken")
    }

    fn device_id(&self) -> Option<&NonEmptyStr> {
        self.gets("DeviceId")
    }

    fn refresh_token(&self) -> Option<&NonEmptyStr> {
        self.gets("RefreshToken")
    }

    fn user_id(&self) -> Option<&NonEmptyStr> {
        self.gets("UserId")
    }

    fn user_key(&self) -> Option<&NonEmptyStr> {
        self.gets("UserKey")
    }

    fn remove_access_token(&mut self) {
        self.dels("AccessToken")
    }

    fn remove_device_id(&mut self) {
        self.dels("DeviceId")
    }

    fn remove_refresh_token(&mut self) {
        self.dels("RefreshToken")
    }

    fn remove_user_id(&mut self) {
        self.dels("UserId")
    }

    fn remove_user_key(&mut self) {
        self.dels("UserKey")
    }

    fn set_access_token<S: Into<String>>(&mut self, v: Option<S>) {
        self.sets("AccessToken", v)
    }

    fn set_device_id<S: Into<String>>(&mut self, v: Option<S>) {
        self.sets("DeviceId", v)
    }

    fn set_refresh_token<S: Into<String>>(&mut self, v: Option<S>) {
        self.sets("RefreshToken", v)
    }

    fn set_user_id<S: Into<String>>(&mut self, v: Option<S>) {
        self.sets("UserId", v)
    }

    fn set_user_key<S: Into<String>>(&mut self, v: Option<S>) {
        self.sets("UserKey", v)
    }

    #[instrument]
    fn save(&self) -> Result<(), Report> {
        if let Some(d) = (*CFG_FILE).parent() {
            std::fs::create_dir_all(d).wrap_err("cannot create configuration dir")?;
        }
        serde_json::to_writer_pretty(
            BufWriter::new(File::create(&*CFG_FILE).wrap_err("cannot create configuration file")?),
            &self.0,
        )
        .wrap_err("cannot create configuration file")
    }
}
