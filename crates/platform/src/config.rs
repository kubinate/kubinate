//! Layered configuration loading.
//!
//! Order of precedence (lowest to highest):
//! 1. Compiled-in defaults.
//! 2. `config/default.toml`, `config/{env}.toml`.
//! 3. Environment variables prefixed `KUBINATE__`.

use serde::Deserialize;

/// Top-level configuration shape. Each binary may extend this with
/// its own nested section.
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    /// Bind address, e.g. `0.0.0.0:8080`.
    pub listen_addr: String,
    /// Postgres DSN.
    pub database_url: String,
    /// Redis URL.
    pub redis_url: String,
    /// OpenTelemetry OTLP endpoint (optional).
    pub otlp_endpoint: Option<String>,
    /// Deployment environment label ("dev", "staging", "prod").
    pub environment: String,
}

impl AppConfig {
    /// Load configuration from environment variables and optional
    /// config files in `./config/`.
    ///
    /// # Errors
    /// Returns a configuration error if mandatory fields are missing
    /// or cannot be deserialized.
    pub fn load() -> Result<Self, config::ConfigError> {
        let env = std::env::var("KUBINATE__ENV").unwrap_or_else(|_| "dev".to_string());

        let config = config::Config::builder()
            .add_source(config::File::with_name("config/default").required(false))
            .add_source(config::File::with_name(&format!("config/{env}")).required(false))
            .add_source(config::Environment::with_prefix("KUBINATE").separator("__"))
            .build()?
            .try_deserialize()?;

        Ok(config)
    }
}
