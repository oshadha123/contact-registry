use serde::Deserialize;

/// All configuration sourced from environment variables.
/// `envy` deserializes and validates these at startup — the process
/// exits immediately if any required variable is missing or malformed.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// TCP port the HTTP server binds to.
    #[serde(default = "default_port")]
    pub port: u16,

    /// Full Postgres connection string.
    /// Example: postgres://user:pass@pgbouncer:5432/registry
    pub database_url: String,

    /// Redis connection URL.
    /// Example: redis://redis:6379
    pub redis_url: String,

    /// Log filter passed to tracing-subscriber's EnvFilter.
    #[serde(default = "default_rust_log")]
    pub rust_log: String,
}

fn default_port()     -> u16    { 8080 }
fn default_rust_log() -> String { "webapp=info,tower_http=info".into() }

impl Config {
    /// Deserialize from the process environment, panic with a clear message on failure.
    pub fn from_env() -> anyhow::Result<Self> {
        envy::from_env::<Config>().map_err(|e| anyhow::anyhow!("Config error: {e}"))
    }
}
