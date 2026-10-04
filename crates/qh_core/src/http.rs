use crate::config::HttpConfig;
use crate::error::BootstrapError;

pub struct HttpClient {
    inner: reqwest::Client,
}

impl HttpClient {
    pub fn new(cfg: &HttpConfig) -> Result<Self, BootstrapError> {
        let client = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .connect_timeout(cfg.connection_timeout)
            .user_agent(&cfg.user_agent)
            .build()
            .map_err(|e| BootstrapError::Runtime(e.to_string()))?;
        Ok(Self { inner: client })
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.inner
    }
}
