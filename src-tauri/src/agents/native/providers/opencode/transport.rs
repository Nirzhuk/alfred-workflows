//! Fixed-loopback HTTP/SSE client for the OpenCode V1 server contract.
//!
//! A workflow cannot supply the listener, credentials, URL, method, or
//! headers. The only production constructor consumes the supervisor's
//! generated password through the trusted bridge in `launch.rs`.

use super::account::OpenCodeGoKey;
use super::protocol::OpenCodePermissionReply;
use crate::agents::native::{NativeCancellation, NativeErrorCode, NativeRuntimeError};
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde_json::{json, Value};
use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;
use zeroize::{Zeroize, Zeroizing};

const SERVER_USERNAME: &str = "opencode";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_JSON_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_ERROR_BODY_BYTES: usize = 32 * 1024;

pub trait OpenCodeSseStream: Send {
    fn next_chunk(
        &mut self,
        cancellation: &NativeCancellation,
    ) -> Result<Option<Vec<u8>>, NativeRuntimeError>;
}

pub trait OpenCodeApi: Send + Sync {
    fn set_go_key(&self, key: &OpenCodeGoKey) -> Result<(), NativeRuntimeError>;
    fn delete_go_key(&self) -> Result<(), NativeRuntimeError>;
    fn list_providers(&self, directory: &str) -> Result<Value, NativeRuntimeError>;
    fn create_session(
        &self,
        directory: &str,
        body: &Value,
        cancellation: &NativeCancellation,
    ) -> Result<Value, NativeRuntimeError>;
    fn get_session(
        &self,
        directory: &str,
        session_id: &str,
        cancellation: &NativeCancellation,
    ) -> Result<Value, NativeRuntimeError>;
    fn delete_session(&self, directory: &str, session_id: &str) -> Result<(), NativeRuntimeError>;
    fn subscribe(
        &self,
        directory: &str,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn OpenCodeSseStream>, NativeRuntimeError>;
    fn prompt_async(
        &self,
        directory: &str,
        session_id: &str,
        body: &Value,
        cancellation: &NativeCancellation,
    ) -> Result<(), NativeRuntimeError>;
    fn reply_permission(
        &self,
        directory: &str,
        request_id: &str,
        reply: OpenCodePermissionReply,
        cancellation: &NativeCancellation,
    ) -> Result<(), NativeRuntimeError>;
    fn abort_session(&self, directory: &str, session_id: &str) -> Result<(), NativeRuntimeError>;
}

/// Password capability handed from the supervisor bridge directly to the
/// backend client. It is neither serializable nor debug-printable.
pub struct OpenCodeServerPassword(Zeroizing<String>);

impl OpenCodeServerPassword {
    pub(crate) fn new(mut value: String) -> Result<Self, NativeRuntimeError> {
        if value.len() < 32
            || value.len() > 256
            || value
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte == b'\0')
        {
            value.zeroize();
            return Err(invalid_transport());
        }
        Ok(Self(Zeroizing::new(value)))
    }

    fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for OpenCodeServerPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpenCodeServerPassword([REDACTED])")
    }
}

pub struct HttpOpenCodeApi {
    client: reqwest::Client,
    base_url: String,
    password: OpenCodeServerPassword,
}

impl HttpOpenCodeApi {
    pub(crate) fn new(
        address: SocketAddr,
        password: OpenCodeServerPassword,
    ) -> Result<Self, NativeRuntimeError> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(invalid_transport());
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            // A loopback capability must never traverse a process- or
            // system-configured HTTP proxy.
            .no_proxy()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|_| unavailable())?;
        Ok(Self {
            client,
            base_url: format!("http://{address}"),
            password,
        })
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.base_url))
            .basic_auth(SERVER_USERNAME, Some(self.password.expose()))
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT)
    }

    fn instance_request(
        &self,
        method: Method,
        path: &str,
        directory: &str,
    ) -> reqwest::RequestBuilder {
        self.request(method, path)
            .query(&[("directory", directory)])
    }
}

impl fmt::Debug for HttpOpenCodeApi {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HttpOpenCodeApi(loopback, [AUTH REDACTED])")
    }
}

impl OpenCodeApi for HttpOpenCodeApi {
    fn set_go_key(&self, key: &OpenCodeGoKey) -> Result<(), NativeRuntimeError> {
        #[derive(Serialize)]
        struct AuthBody<'a> {
            #[serde(rename = "type")]
            kind: &'static str,
            key: &'a str,
        }
        let body = AuthBody {
            kind: "api",
            key: key.expose(),
        };
        let response = send(
            self.request(Method::PUT, "/auth/opencode-go").json(&body),
            None,
        )?;
        expect_empty_or_boolean(response, StatusCode::OK)
    }

    fn delete_go_key(&self) -> Result<(), NativeRuntimeError> {
        let response = send(self.request(Method::DELETE, "/auth/opencode-go"), None)?;
        expect_empty_or_boolean(response, StatusCode::OK)
    }

    fn list_providers(&self, directory: &str) -> Result<Value, NativeRuntimeError> {
        let response = send(
            self.instance_request(Method::GET, "/provider", directory),
            None,
        )?;
        read_json(response, StatusCode::OK)
    }

    fn create_session(
        &self,
        directory: &str,
        body: &Value,
        cancellation: &NativeCancellation,
    ) -> Result<Value, NativeRuntimeError> {
        let response = send(
            self.instance_request(Method::POST, "/session", directory)
                .json(body),
            Some(cancellation),
        )?;
        read_json(response, StatusCode::OK)
    }

    fn get_session(
        &self,
        directory: &str,
        session_id: &str,
        cancellation: &NativeCancellation,
    ) -> Result<Value, NativeRuntimeError> {
        validate_wire_id(session_id)?;
        let response = send(
            self.instance_request(Method::GET, &format!("/session/{session_id}"), directory),
            Some(cancellation),
        )?;
        if response.status() == StatusCode::NOT_FOUND {
            discard_bounded_body(response, MAX_ERROR_BODY_BYTES);
            return Err(session_unavailable());
        }
        read_json(response, StatusCode::OK)
    }

    fn delete_session(&self, directory: &str, session_id: &str) -> Result<(), NativeRuntimeError> {
        validate_wire_id(session_id)?;
        let response = send(
            self.instance_request(Method::DELETE, &format!("/session/{session_id}"), directory),
            None,
        )?;
        if response.status() == StatusCode::NOT_FOUND {
            discard_bounded_body(response, MAX_ERROR_BODY_BYTES);
            return Err(session_unavailable());
        }
        expect_empty_or_boolean(response, StatusCode::OK)
    }

    fn subscribe(
        &self,
        directory: &str,
        cancellation: &NativeCancellation,
    ) -> Result<Box<dyn OpenCodeSseStream>, NativeRuntimeError> {
        cancellation.checkpoint()?;
        let request = self
            .instance_request(Method::GET, "/event", directory)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .timeout(Duration::from_secs(24 * 60 * 60));
        let response = send(request, Some(cancellation))?;
        classify_status(response.status())?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !content_type
            .to_ascii_lowercase()
            .starts_with("text/event-stream")
        {
            return Err(protocol_error());
        }
        Ok(Box::new(ReqwestOpenCodeSseStream {
            response: Some(response),
        }))
    }

    fn prompt_async(
        &self,
        directory: &str,
        session_id: &str,
        body: &Value,
        cancellation: &NativeCancellation,
    ) -> Result<(), NativeRuntimeError> {
        validate_wire_id(session_id)?;
        let response = send(
            self.instance_request(
                Method::POST,
                &format!("/session/{session_id}/prompt_async"),
                directory,
            )
            .json(body),
            Some(cancellation),
        )?;
        if response.status() == StatusCode::NOT_FOUND {
            discard_bounded_body(response, MAX_ERROR_BODY_BYTES);
            return Err(session_unavailable());
        }
        expect_empty_or_boolean(response, StatusCode::NO_CONTENT)
    }

    fn reply_permission(
        &self,
        directory: &str,
        request_id: &str,
        reply: OpenCodePermissionReply,
        cancellation: &NativeCancellation,
    ) -> Result<(), NativeRuntimeError> {
        validate_wire_id(request_id)?;
        let body = json!({"reply": reply.as_str()});
        let response = send(
            self.instance_request(
                Method::POST,
                &format!("/permission/{request_id}/reply"),
                directory,
            )
            .json(&body),
            Some(cancellation),
        )?;
        expect_empty_or_boolean(response, StatusCode::OK)
    }

    fn abort_session(&self, directory: &str, session_id: &str) -> Result<(), NativeRuntimeError> {
        validate_wire_id(session_id)?;
        let response = send(
            self.instance_request(
                Method::POST,
                &format!("/session/{session_id}/abort"),
                directory,
            ),
            None,
        )?;
        if response.status() == StatusCode::NOT_FOUND {
            discard_bounded_body(response, MAX_ERROR_BODY_BYTES);
            return Err(session_unavailable());
        }
        expect_empty_or_boolean(response, StatusCode::OK)
    }
}

struct ReqwestOpenCodeSseStream {
    response: Option<reqwest::Response>,
}

impl OpenCodeSseStream for ReqwestOpenCodeSseStream {
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

fn send(
    request: reqwest::RequestBuilder,
    cancellation: Option<&NativeCancellation>,
) -> Result<reqwest::Response, NativeRuntimeError> {
    if let Some(cancellation) = cancellation {
        cancellation.checkpoint()?;
    }
    tauri::async_runtime::block_on(async {
        let send = request.send();
        tokio::pin!(send);
        loop {
            tokio::select! {
                result = &mut send => break result.map_err(map_reqwest_error),
                _ = tokio::time::sleep(Duration::from_millis(10)), if cancellation.is_some() => {
                    cancellation.expect("guarded").checkpoint()?;
                }
            }
        }
    })
}

fn read_json(
    response: reqwest::Response,
    expected: StatusCode,
) -> Result<Value, NativeRuntimeError> {
    let status = response.status();
    if status != expected {
        discard_bounded_body(response, MAX_ERROR_BODY_BYTES);
        return Err(classify_status_error(status));
    }
    let body = read_bounded_body(response, MAX_JSON_BODY_BYTES)?;
    serde_json::from_slice(&body).map_err(|_| protocol_error())
}

fn expect_empty_or_boolean(
    response: reqwest::Response,
    expected: StatusCode,
) -> Result<(), NativeRuntimeError> {
    let status = response.status();
    if status != expected {
        discard_bounded_body(response, MAX_ERROR_BODY_BYTES);
        return Err(classify_status_error(status));
    }
    let mut body = read_bounded_body(response, 64)?;
    let accepted = body.is_empty() || body == b"true";
    body.zeroize();
    if accepted {
        Ok(())
    } else {
        Err(protocol_error())
    }
}

fn discard_bounded_body(response: reqwest::Response, maximum: usize) {
    if let Ok(mut body) = read_bounded_body(response, maximum) {
        body.zeroize();
    }
}

fn read_bounded_body(
    mut response: reqwest::Response,
    maximum: usize,
) -> Result<Vec<u8>, NativeRuntimeError> {
    tauri::async_runtime::block_on(async {
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
            if body.len().saturating_add(chunk.len()) > maximum {
                body.zeroize();
                return Err(NativeRuntimeError::new(
                    NativeErrorCode::EventLimitExceeded,
                    "OpenCode response exceeded its bounded body limit",
                    false,
                ));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    })
}

fn classify_status(status: StatusCode) -> Result<(), NativeRuntimeError> {
    if status == StatusCode::OK {
        Ok(())
    } else {
        Err(classify_status_error(status))
    }
}

fn classify_status_error(status: StatusCode) -> NativeRuntimeError {
    match status.as_u16() {
        401 | 403 => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "the authenticated managed OpenCode listener rejected Alfred",
            false,
        ),
        402 | 429 => NativeRuntimeError::new(
            NativeErrorCode::ProviderUnavailable,
            "OpenCode Go usage limit reached; usage is available only in the OpenCode console",
            true,
        ),
        404 => unavailable(),
        408 => NativeRuntimeError::timed_out(),
        _ if status.is_server_error() => unavailable(),
        _ => protocol_error(),
    }
}

fn validate_wire_id(value: &str) -> Result<(), NativeRuntimeError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'));
    if valid {
        Ok(())
    } else {
        Err(protocol_error())
    }
}

fn map_reqwest_error(error: reqwest::Error) -> NativeRuntimeError {
    if error.is_timeout() {
        NativeRuntimeError::timed_out()
    } else {
        unavailable()
    }
}

fn invalid_transport() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::InvalidRequest,
        "OpenCode trusted loopback transport is invalid",
        false,
    )
}

fn protocol_error() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::InvalidEvent,
        "OpenCode server response did not match the pinned V1 contract",
        false,
    )
}

fn session_unavailable() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::SessionUnavailable,
        "OpenCode session is unavailable",
        false,
    )
}

fn unavailable() -> NativeRuntimeError {
    NativeRuntimeError::new(
        NativeErrorCode::ProviderUnavailable,
        "the managed OpenCode server is unavailable",
        true,
    )
}
