//! HTTP transport for the Anthropic API.
//!
//! The trait exists so the turn loop is exercised by fixtures without a
//! network. Only the documented endpoints and headers are used; no Claude CLI
//! binary, credential file, or undocumented endpoint is ever consulted.

use super::wire::{classify_status, ANTHROPIC_VERSION, MESSAGES_URL, MODELS_URL};
use crate::agents::native::{NativeCancellation, NativeErrorCode, NativeRuntimeError};
use serde_json::Value;
use std::time::Duration;

pub const CLAUDE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const CLAUDE_READ_TIMEOUT: Duration = Duration::from_secs(30);
pub const CLAUDE_REQUEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const MAX_MODEL_CATALOG_BYTES: usize = 2 * 1024 * 1024;
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// A byte stream that yields SSE chunks, one blocking read at a time so the
/// turn loop can checkpoint cancellation between chunks.
pub trait ClaudeByteStream: Send {
    fn next_chunk(
        &mut self,
        cancellation: &NativeCancellation,
    ) -> Result<Option<Vec<u8>>, NativeRuntimeError>;
}

pub trait ClaudeTransport: Send + Sync {
    fn stream_messages(
        &self,
        api_key: &str,
        body: &Value,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn ClaudeByteStream>, NativeRuntimeError>;

    fn list_models(&self, api_key: &str) -> Result<String, NativeRuntimeError>;
}

pub struct HttpClaudeTransport {
    client: reqwest::Client,
    messages_url: String,
    models_url: String,
}

impl HttpClaudeTransport {
    #[allow(dead_code)]
    pub(super) fn new() -> Result<Self, NativeRuntimeError> {
        Self::with_endpoints(MESSAGES_URL, MODELS_URL)
    }

    fn with_endpoints(messages_url: &str, models_url: &str) -> Result<Self, NativeRuntimeError> {
        let client = tauri::async_runtime::block_on(async {
            reqwest::Client::builder()
                // The API key is carried in a custom `x-api-key` header. Reqwest
                // does not strip that header on a cross-host redirect, so redirects
                // must be disabled rather than merely host-checked afterwards.
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(CLAUDE_CONNECT_TIMEOUT)
                .read_timeout(CLAUDE_READ_TIMEOUT)
                .timeout(CLAUDE_REQUEST_TIMEOUT)
                .build()
        })
        .map_err(|_| unavailable("Anthropic HTTP client could not be created"))?;
        Ok(Self {
            client,
            messages_url: messages_url.into(),
            models_url: models_url.into(),
        })
    }

    #[cfg(test)]
    pub(super) fn fixture(
        messages_url: &str,
        models_url: &str,
    ) -> Result<Self, NativeRuntimeError> {
        Self::with_endpoints(messages_url, models_url)
    }
}

impl ClaudeTransport for HttpClaudeTransport {
    fn stream_messages(
        &self,
        api_key: &str,
        body: &Value,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn ClaudeByteStream>, NativeRuntimeError> {
        cancellation.checkpoint()?;
        let request = self
            .client
            .post(&self.messages_url)
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(body);
        let response = tauri::async_runtime::block_on(async move {
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
        let status = response.status().as_u16();
        if status != 200 {
            let body = read_bounded_body(response, MAX_ERROR_BODY_BYTES).unwrap_or_default();
            return Err(classify_status(status, &body).error());
        }
        Ok(Box::new(ReqwestByteStream {
            response: Some(response),
        }))
    }

    fn list_models(&self, api_key: &str) -> Result<String, NativeRuntimeError> {
        let request = self
            .client
            .get(&self.models_url)
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION);
        let response = tauri::async_runtime::block_on(async move { request.send().await })
            .map_err(map_reqwest_error)?;
        let status = response.status().as_u16();
        let body = read_bounded_body(
            response,
            if status == 200 {
                MAX_MODEL_CATALOG_BYTES
            } else {
                MAX_ERROR_BODY_BYTES
            },
        )?;
        if status != 200 {
            return Err(classify_status(status, &body).error());
        }
        Ok(body)
    }
}

struct ReqwestByteStream {
    response: Option<reqwest::Response>,
}

impl ClaudeByteStream for ReqwestByteStream {
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

fn read_bounded_body(
    response: reqwest::Response,
    maximum: usize,
) -> Result<String, NativeRuntimeError> {
    tauri::async_runtime::block_on(async {
        let mut response = response;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
            if body.len().saturating_add(chunk.len()) > maximum {
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::EventLimitExceeded,
                    "Anthropic response body exceeded its byte limit",
                    false,
                ));
            }
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).map_err(|_| {
            NativeRuntimeError::new(
                NativeErrorCode::ProviderUnavailable,
                "Anthropic response body was not UTF-8",
                false,
            )
        })
    })
}

fn map_reqwest_error(error: reqwest::Error) -> NativeRuntimeError {
    if error.is_timeout() {
        NativeRuntimeError::timed_out()
    } else {
        unavailable("Anthropic API could not be reached")
    }
}

fn unavailable(message: &str) -> NativeRuntimeError {
    NativeRuntimeError::new(NativeErrorCode::ProviderUnavailable, message, true)
}
