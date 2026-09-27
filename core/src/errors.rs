use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;
use utoipa::ToSchema;
use tracing::error;

use crate::comparison::ComparisonError;
use crate::simulation::SimulationError;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum AppError {
    #[error("Internal server error")]
    Internal(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Too many requests: {0}")]
    TooManyRequests(String),
}

/// RFC 7807 "Problem Details for HTTP APIs" response body.
#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    /// A URI reference that identifies the problem type
    #[schema(example = "https://Sky Moon Scope.dev/errors/not-found")]
    r#type: String,
    /// A short, human-readable summary of the problem type
    #[schema(example = "Not Found")]
    title: String,
    /// The HTTP status code for this occurrence of the problem
    #[schema(example = 404)]
    status: u16,
    /// A human-readable explanation specific to this occurrence of the problem
    detail: String,
    /// A URI reference identifying this specific occurrence of the problem
    #[serde(skip_serializing_if = "Option::is_none")]
    instance: Option<String>,
}

impl AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
        }
    }

    fn error_type(&self) -> &str {
        match self {
            Self::Internal(_) => "internal-server-error",
            Self::NotFound(_) => "not-found",
            Self::BadRequest(_) => "bad-request",
            Self::Unauthorized(_) => "unauthorized",
            Self::TooManyRequests(_) => "too-many-requests",
        }
    }

    fn title(&self) -> &str {
        match self {
            Self::Internal(_) => "Internal Server Error",
            Self::NotFound(_) => "Not Found",
            Self::BadRequest(_) => "Bad Request",
            Self::Unauthorized(_) => "Unauthorized",
            Self::TooManyRequests(_) => "Too Many Requests",
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();

        // Log internal errors with full Debug details server-side
        if status.is_server_error() {
            error!(
                error_type = self.error_type(),
                status = status.as_u16(),
                error_message = %self,
                error_debug = ?self,
                "Server error response: {}",
                self
            );
            
            // Client receives opaque error message
            let body = Json(ErrorResponse {
                r#type: format!(
                    "https://Sky Moon Scope.dev/errors/{}",
                    self.error_type()
                ),
                title: self.title().to_string(),
                status: status.as_u16(),
                // Return opaque error message for server errors (5xx)
                detail: "An internal server error occurred. Please try again later.".to_string(),
                instance: None,
            });

            let mut response = (status, body).into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/problem+json"),
            );
            response
        } else {
            // For client errors (4xx), preserve the original error message
            // since it typically reflects user-controlled input validation
            let body = Json(ErrorResponse {
                r#type: format!(
                    "https://Sky Moon Scope.dev/errors/{}",
                    self.error_type()
                ),
                title: self.title().to_string(),
                status: status.as_u16(),
                detail: self.to_string(),
                instance: None,
            });

            let mut response = (status, body).into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/problem+json"),
            );
            response
        }
    }
}

/// Convert SimulationError to AppError with appropriate HTTP status codes.
///
/// Maps client errors (4xx) to BadRequest and server errors (5xx) to Internal.
impl From<SimulationError> for AppError {
    fn from(err: SimulationError) -> Self {
        match err {
            // Client errors (HTTP 400)
            SimulationError::NodeError(msg) => {
                // NodeError covers invalid contract IDs, bad parameters
                AppError::BadRequest(format!("RPC node error: {}", msg))
            }
            SimulationError::InvalidContract(msg) => {
                AppError::BadRequest(format!("Invalid contract: {}", msg))
            }
            SimulationError::ParseError(e) => {
                AppError::BadRequest(format!("Argument parse error: {}", e))
            }
            SimulationError::XdrError(msg) => {
                AppError::BadRequest(format!("XDR encoding error: {}", msg))
            }
            SimulationError::Base64Error(e) => {
                AppError::BadRequest(format!("Base64 decode error: {}", e))
            }

            // Server errors (HTTP 500)
            SimulationError::NodeTimeout => AppError::Internal("RPC request timed out".to_string()),
            SimulationError::RpcRequestFailed(msg) => {
                AppError::Internal(format!("RPC request failed: {}", msg))
            }
            SimulationError::NetworkError(e) => AppError::Internal(format!("Network error: {}", e)),
            SimulationError::Io(e) => AppError::Internal(format!("IO error: {}", e)),
            SimulationError::SerializationError(e) => {
                AppError::Internal(format!("Serialization error: {}", e))
            }

            // Local-runner errors. `LocalUnavailable` should normally be
            // handled upstream by falling back to RPC, so if it reaches the
            // HTTP boundary treat it as an internal misconfiguration.
            SimulationError::LocalUnavailable => AppError::Internal(
                "Local WASM execution unavailable and no RPC fallback succeeded".to_string(),
            ),
            SimulationError::ExecutionFailed(msg) => {
                AppError::BadRequest(format!("Contract execution failed: {}", msg))
            }
            SimulationError::InsufficientConsensusProviders(msg) => {
                AppError::Internal(format!("Insufficient consensus providers: {}", msg))
            }
            SimulationError::ConsensusMismatch(msg) => {
                AppError::Internal(format!("Consensus mismatch: {}", msg))
            }
        }
    }
}

/// Convert a [`ComparisonError`] to an [`AppError`].
///
/// A missing baseline snapshot is a client-visible "not found" condition — the
/// caller asked to diff against a snapshot that does not exist — so it maps to
/// [`AppError::NotFound`] (HTTP 404) rather than a generic 500. Simulation
/// failures delegate to the existing [`SimulationError`] mapping.
impl From<ComparisonError> for AppError {
    fn from(err: ComparisonError) -> Self {
        match err {
            ComparisonError::BaselineNotFound(snapshot_id) => {
                AppError::NotFound(format!("Baseline snapshot not found: {}", snapshot_id))
            }
            ComparisonError::Simulation(inner) => AppError::from(inner),
        }
    }
}
