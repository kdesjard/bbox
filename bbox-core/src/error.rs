//! Error and Result types.
use actix_web::{http::StatusCode, ResponseError};
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Error {
    #[error("Geometry format error")]
    GeometryFormatError,
    #[error("datasource setup error - {0}")]
    DatasourceSetupError(String),
    #[error("datasource `{0}` not found")]
    DatasourceNotFound(String),
    // Database errors
    #[error(transparent)]
    DbError(#[from] sqlx::Error),
    //
    #[error("Query parameters error")]
    QueryParams,
    #[error("Invalid collection")]
    InvalidCollection,
    #[error("HTML Templating")]
    HtmlTemplate,
    //
    #[error("No node found")]
    NodeNotFound,
    #[error("No route found")]
    NoRouteFound,
    // Requests
    #[error("Argument error `{0}`")]
    ArgumentError(String),
    // General
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error("Bincode error")]
    BincodeError(#[from] bincode::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl ResponseError for Error {
    fn status_code(&self) -> StatusCode {
        match *self {
            Self::QueryParams | Self::GeometryFormatError => StatusCode::BAD_REQUEST,
            Self::InvalidCollection => StatusCode::NOT_FOUND,
            Self::HtmlTemplate => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
