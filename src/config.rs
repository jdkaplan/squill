use std::path::PathBuf;

use sqlx::{postgres::PgConnectOptions, ConnectOptions, PgConnection};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_connect_options: Option<PgConnectOptions>,

    pub migrations_dir: PathBuf,
    pub templates_dir: Option<PathBuf>,
}

impl Config {
    pub async fn connect(&self) -> Result<PgConnection, ConnectError> {
        if let Some(opts) = &self.database_connect_options {
            opts.connect().await.map_err(ConnectError::Connect)
        } else {
            Err(ConnectError::NotConfigured)
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ConnectError {
    #[error("no database configured")]
    NotConfigured,

    #[error("failed to connect to database: {0}")]
    Connect(sqlx::Error),
}

#[cfg(test)]
mod tests {
    use crate::testing::*;

    use super::*;

    #[tokio::test]
    async fn connect() {
        let env = TestEnv::new().await.unwrap();

        let config = env.config();

        config.connect().await.unwrap();
    }

    #[tokio::test]
    async fn not_configured() {
        let env = TestEnv::new().await.unwrap();

        let mut config = env.config();
        config.database_connect_options = None;

        let res = config.connect().await;
        assert!(res.is_err());

        match res.unwrap_err() {
            ConnectError::NotConfigured => (),
            err => panic!("Unexpected error: {:?}", err),
        };
    }

    #[tokio::test]
    async fn connect_error() {
        let env = TestEnv::new().await.unwrap();

        let mut config = env.config();
        config.database_connect_options = config
            .database_connect_options
            .map(|opts| opts.database("__not_a_squill_test"));

        let res = config.connect().await;
        assert!(res.is_err());

        match res.unwrap_err() {
            ConnectError::Connect(_) => (),
            err => panic!("Unexpected error: {:?}", err),
        };
    }
}
