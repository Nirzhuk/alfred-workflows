//! Fixed-host HTTP transport for the official Gemini Developer API.
//!
//! The workflow supplies no URL or headers. The only secret-bearing header is
//! `x-goog-api-key`, populated from the resolved Plan 031 credential.

use super::credential::GeminiCredential;
use super::protocol::error_for_status;
use super::{GEMINI_API_HOST, GEMINI_API_KEY_HEADER, GEMINI_API_VERSION};
use crate::agents::native::{NativeCancellation, NativeErrorCode, NativeRuntimeError};
use serde_json::Value;
use std::time::Duration;

const MODEL_LIST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_MODEL_CATALOG_BYTES: usize = 2 * 1024 * 1024;

/// A response byte stream whose blocking reads remain cancellation-aware.
pub trait GeminiByteStream: Send {
    fn next_chunk(
        &mut self,
        cancellation: &NativeCancellation,
    ) -> Result<Option<Vec<u8>>, NativeRuntimeError>;
}

pub trait GeminiTransport: Send + Sync {
    fn stream_generate(
        &self,
        credential: &GeminiCredential,
        model: &str,
        body: &Value,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn GeminiByteStream>, NativeRuntimeError>;

    fn list_models(
        &self,
        credential: &GeminiCredential,
    ) -> Result<String, NativeRuntimeError>;
}

pub struct HttpGeminiTransport {
    client: reqwest::Client,
}

impl HttpGeminiTransport {
    pub fn new() -> Result<Self, NativeRuntimeError> {
        reqwest::Client::builder()
            // A fixed API host must never be redirected to a credential sink.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map(|client| Self { client })
            .map_err(|_| unavailable("gemini HTTP client could not be created"))
    }
}

impl GeminiTransport for HttpGeminiTransport {
    fn stream_generate(
        &self,
        credential: &GeminiCredential,
        model: &str,
        body: &Value,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn GeminiByteStream>, NativeRuntimeError> {
        validate_model_path(model)?;
        cancellation.checkpoint()?;
        let url = format!(
            "{GEMINI_API_HOST}/{GEMINI_API_VERSION}/models/{model}:streamGenerateContent"
        );
        let request = self
            .client
            .post(url)
            .query(&[("alt", "sse")])
            .header(GEMINI_API_KEY_HEADER, credential.header_value())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(body);

        let response = tauri::async_runtime::block_on(async {
            let send = request.send();
            tokio::pin!(send);
            loop {
                tokio::select! {
                    result = &mut send => break result.map_err(map_reqwest_error),
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {
                        cancellation.checkpoint()?;
                    }
                }
            }
        })?;
        let status = response.status();
        if status.as_u16() != 200 {
            return Err(error_for_status(
                status.as_u16(),
                status.canonical_reason().unwrap_or("request failed"),
            ));
        }
        Ok(Box::new(ReqwestGeminiStream {
            response: Some(response),
        }))
    }

    fn list_models(
        &self,
        credential: &GeminiCredential,
    ) -> Result<String, NativeRuntimeError> {
        let url = format!("{GEMINI_API_HOST}/{GEMINI_API_VERSION}/models");
        let request = self
            .client
            .get(url)
            // The documented endpoint allows up to 1000 per page. Asking for
            // that maximum avoids silently publishing only its default 50.
            .query(&[("pageSize", "1000")])
            .timeout(MODEL_LIST_TIMEOUT)
            .header(GEMINI_API_KEY_HEADER, credential.header_value());
        let response = tauri::async_runtime::block_on(request.send()).map_err(map_reqwest_error)?;
        let status = response.status();
        if status.as_u16() != 200 {
            return Err(error_for_status(
                status.as_u16(),
                status.canonical_reason().unwrap_or("request failed"),
            ));
        }
        tauri::async_runtime::block_on(async {
            let mut response = response;
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| unavailable("gemini model catalog could not be read"))?
            {
                if body.len().saturating_add(chunk.len()) > MAX_MODEL_CATALOG_BYTES {
                    return Err(NativeRuntimeError::new(
                        NativeErrorCode::EventLimitExceeded,
                        "gemini model catalog exceeded its byte limit",
                        false,
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            String::from_utf8(body).map_err(|_| {
                NativeRuntimeError::new(
                    NativeErrorCode::ModelUnavailable,
                    "gemini model catalog was not UTF-8",
                    false,
                )
            })
        })
    }
}

struct ReqwestGeminiStream {
    response: Option<reqwest::Response>,
}

impl GeminiByteStream for ReqwestGeminiStream {
    fn next_chunk(
        &mut self,
        cancellation: &NativeCancellation,
    ) -> Result<Option<Vec<u8>>, NativeRuntimeError> {
        cancellation.checkpoint()?;
        let Some(response) = self.response.as_mut() else {
            return Ok(None);
        };
        let next = tauri::async_runtime::block_on(async {
            let chunk = response.chunk();
            tokio::pin!(chunk);
            loop {
                tokio::select! {
                    result = &mut chunk => break result.map_err(map_reqwest_error),
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {
                        cancellation.checkpoint()?;
                    }
                }
            }
        });
        match next {
            Ok(Some(chunk)) => Ok(Some(chunk.to_vec())),
            Ok(None) => {
                self.response = None;
                Ok(None)
            }
            Err(error) => {
                self.response = None;
                Err(error)
            }
        }
    }
}

fn validate_model_path(model: &str) -> Result<(), NativeRuntimeError> {
    let valid = !model.is_empty()
        && model.len() <= 256
        && model.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
        });
    if valid {
        Ok(())
    } else {
        Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "gemini model identifier is not safe for the fixed API path",
            false,
        ))
    }
}

fn map_reqwest_error(error: reqwest::Error) -> NativeRuntimeError {
    if error.is_timeout() {
        NativeRuntimeError::timed_out()
    } else {
        unavailable("gemini API could not be reached")
    }
}

fn unavailable(message: &str) -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::ProviderUnavailable, message, true)
}
