use crate::config::DsPostgisCfg;
use log::info;
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    DbError(#[from] sqlx::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub struct PgDatasource {
    pub pool: PgPool,
    pub schemas: Vec<String>,
}

impl PgDatasource {
    pub async fn from_config(ds: &DsPostgisCfg, envvar: Option<String>) -> Result<Self> {
        Self::new_pool(&envvar.unwrap_or(ds.url.clone()), ds.search_path.clone()).await
    }
    pub async fn new_pool(url: &str, search_path: Option<String>) -> Result<Self> {
        info!("Connecting to {url}");
        let connect_options = PgConnectOptions::new();
        let schemas = if let Some(sp) = search_path {
            info!("Setting search_path to {sp}");
            connect_options.options([("search_path", sp.clone())]);
            sp.split(',').map(str::to_string).collect()
        } else {
            vec!["public".to_string()]
        };
        let pool = PgPoolOptions::new()
            .min_connections(0)
            .max_connections(8)
            .connect(url)
            .await?;
        Ok(PgDatasource { pool, schemas })
    }
}
