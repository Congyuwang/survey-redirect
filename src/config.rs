use config::{Config as Conf, ConfigError};
use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};
use url::Url;

use crate::CONFIG_FILE_NAME;

#[derive(Deserialize)]
pub struct Config {
    pub server_binding: SocketAddr,
    pub base_url: Url,
    pub admin_token: String,
    pub storage_root: PathBuf,
    pub log_file: PathBuf,
    pub server_tls: Option<TlsConfig>,
}

#[derive(Deserialize)]
pub struct TlsConfig {
    pub key: PathBuf,
    pub cert: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let config = Conf::builder()
            .add_source(config::File::with_name(CONFIG_FILE_NAME))
            .build()?;
        config.try_deserialize()
    }
}
