//! Error and Result types.
use actix_web::{
    body::MessageBody,
    dev::ServiceResponse,
    error::{Error as WebError, InternalError},
    http::StatusCode,
    middleware::ErrorHandlerResponse,
    HttpResponse,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub code: u16,
    pub message: String,
    pub error: String,
}

/// # Errors
///
/// middleware error handler can return an error, but this one does not
pub fn http_error_handler<B: MessageBody, E>(
    res: ServiceResponse<B>,
) -> Result<ErrorHandlerResponse<B>, E> {
    let (request, response) = res.into_parts();

    let default_error = WebError::from(InternalError::new(
        "Unknown",
        StatusCode::INTERNAL_SERVER_ERROR,
    ));

    let error = response.error().unwrap_or(&default_error);

    let code = response.status().as_u16();
    let error_str = match code {
        404 => "URL Not Found".to_string(),
        429 => "Too Many Requests".to_string(),
        500..=599 => "Internal".to_string(),
        _ => error.to_string(),
    };
    #[allow(clippy::redundant_closure_for_method_calls)]
    let error_response = ErrorResponse {
        code: response.status().as_u16(),
        message: response.status().to_string(),
        error: error_str,
    };
    log::info!("{:?}", error_response);

    let mut new_response = HttpResponse::build(response.status())
        .content_type("application/json")
        .json(error_response);

    let headers = new_response.headers_mut();
    for (key, val) in response.headers() {
        headers.insert(key.to_owned(), val.to_owned());
    }

    let res = ServiceResponse::new(request, new_response).map_into_right_body();
    Ok(ErrorHandlerResponse::Response(res))
}
