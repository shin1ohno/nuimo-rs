/// Configuration from environment variables.
pub struct Config {
    pub broker_url: String,
    pub broker_user: Option<String>,
    pub broker_password: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            broker_url: std::env::var("BROKER_URL")
                .unwrap_or_else(|_| "mqtt://localhost:1883".into()),
            broker_user: std::env::var("BROKER_USER_NAME").ok(),
            broker_password: std::env::var("BROKER_PASSWORD").ok(),
        }
    }
}
