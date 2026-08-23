use chrono::{DateTime, Utc};
use reqwest::{redirect::Policy, Client, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use url::Url;
use uuid::Uuid;

use super::config::PolarConfig;

const ACTIVATE_PATH: &str = "v1/customer-portal/license-keys/activate";
const VALIDATE_PATH: &str = "v1/customer-portal/license-keys/validate";
const DEACTIVATE_PATH: &str = "v1/customer-portal/license-keys/deactivate";
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolarLicenseState {
    Granted,
    Revoked,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolarLicenseResult {
    pub benefit_id: Uuid,
    pub status: PolarLicenseState,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolarActivationResult {
    pub activation_id: Uuid,
    pub label: String,
    pub license: PolarLicenseResult,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PolarClientError {
    #[error("the license could not be found")]
    InvalidLicense,
    #[error("the license does not support device activations")]
    ActivationsUnsupported,
    #[error("the activation limit was reached")]
    DeviceLimit,
    #[error("the Polar request timed out")]
    Timeout,
    #[error("Polar could not be reached")]
    Connectivity,
    #[error("Polar rate limited the request")]
    RateLimited,
    #[error("Polar is temporarily unavailable")]
    ServiceUnavailable,
    #[error("Polar returned too much data")]
    ResponseTooLarge,
    #[error("Polar returned an invalid response")]
    InvalidResponse,
}

impl PolarClientError {
    pub fn is_transient(self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::Connectivity | Self::RateLimited | Self::ServiceUnavailable
        )
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidLicense => "invalid_license",
            Self::ActivationsUnsupported => "activations_unsupported",
            Self::DeviceLimit => "device_limit",
            Self::Timeout => "polar_timeout",
            Self::Connectivity => "polar_connectivity",
            Self::RateLimited => "polar_rate_limited",
            Self::ServiceUnavailable => "polar_unavailable",
            Self::ResponseTooLarge => "polar_response_too_large",
            Self::InvalidResponse => "polar_invalid_response",
        }
    }
}

#[derive(Clone)]
pub struct PolarClient {
    client: Client,
    api_base: Url,
    organization_id: Uuid,
}

impl PolarClient {
    pub fn new(config: &PolarConfig) -> Result<Self, PolarClientError> {
        Self::with_timeouts(config, DEFAULT_CONNECT_TIMEOUT, DEFAULT_REQUEST_TIMEOUT)
    }

    fn with_timeouts(
        config: &PolarConfig,
        connect_timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Self, PolarClientError> {
        let client = Client::builder()
            .connect_timeout(connect_timeout)
            .timeout(request_timeout)
            // A redirect must never carry a customer key to another origin.
            .redirect(Policy::none())
            .build()
            .map_err(|_| PolarClientError::InvalidResponse)?;
        Ok(Self {
            client,
            api_base: config.api_base.clone(),
            organization_id: config.organization_id,
        })
    }

    pub async fn activate(
        &self,
        key: &str,
        label: &str,
    ) -> Result<PolarActivationResult, PolarClientError> {
        let body = ActivateRequest {
            key,
            organization_id: self.organization_id,
            label,
        };
        let response = self.post(ACTIVATE_PATH, &body).await?;
        let response: ActivateResponse = read_json(response).await?;
        Ok(PolarActivationResult {
            activation_id: response.id,
            label: response.label,
            license: response.license_key.into(),
        })
    }

    pub async fn validate(
        &self,
        key: &str,
        activation_id: Option<Uuid>,
    ) -> Result<PolarLicenseResult, PolarClientError> {
        let body = ValidateRequest {
            key,
            organization_id: self.organization_id,
            activation_id,
        };
        let response = self.post(VALIDATE_PATH, &body).await?;
        let response: LicenseResponse = read_json(response).await?;
        Ok(response.into())
    }

    pub async fn deactivate(&self, key: &str, activation_id: Uuid) -> Result<(), PolarClientError> {
        let body = DeactivateRequest {
            key,
            organization_id: self.organization_id,
            activation_id,
        };
        let response = self.post(DEACTIVATE_PATH, &body).await?;
        if response.status() == StatusCode::NO_CONTENT {
            Ok(())
        } else {
            Err(PolarClientError::InvalidResponse)
        }
    }

    async fn post<T: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<Response, PolarClientError> {
        let endpoint = self
            .api_base
            .join(path)
            .map_err(|_| PolarClientError::InvalidResponse)?;
        let response = self
            .client
            .post(endpoint)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        match response.status() {
            status if status.is_success() => Ok(response),
            StatusCode::FORBIDDEN => {
                let detail = read_error_detail(response).await;
                if detail.is_some_and(|detail| activations_are_unsupported(&detail)) {
                    Err(PolarClientError::ActivationsUnsupported)
                } else {
                    Err(PolarClientError::DeviceLimit)
                }
            }
            StatusCode::NOT_FOUND | StatusCode::UNPROCESSABLE_ENTITY => {
                Err(PolarClientError::InvalidLicense)
            }
            StatusCode::TOO_MANY_REQUESTS => Err(PolarClientError::RateLimited),
            status if status.is_server_error() => Err(PolarClientError::ServiceUnavailable),
            _ => Err(PolarClientError::InvalidResponse),
        }
    }
}

fn map_reqwest_error(error: reqwest::Error) -> PolarClientError {
    if error.is_timeout() {
        PolarClientError::Timeout
    } else if error.is_connect() || error.is_request() || error.is_body() {
        PolarClientError::Connectivity
    } else {
        PolarClientError::InvalidResponse
    }
}

async fn read_json<T: DeserializeOwned>(mut response: Response) -> Result<T, PolarClientError> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err(PolarClientError::ResponseTooLarge);
    }

    let mut payload = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
        if payload.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(PolarClientError::ResponseTooLarge);
        }
        payload.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&payload).map_err(|_| PolarClientError::InvalidResponse)
}

async fn read_error_detail(mut response: Response) -> Option<String> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return None;
    }

    let mut payload = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if payload.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return None;
        }
        payload.extend_from_slice(&chunk);
    }

    let value: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    ["detail", "error"]
        .into_iter()
        .find_map(|field| value.get(field).and_then(serde_json::Value::as_str))
        .map(str::to_owned)
}

fn activations_are_unsupported(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    detail.contains("does not support activations")
        || detail.contains("activation not supported")
        || detail.contains("activations not supported")
}

#[derive(Serialize)]
struct ActivateRequest<'a> {
    key: &'a str,
    organization_id: Uuid,
    label: &'a str,
}

#[derive(Serialize)]
struct ValidateRequest<'a> {
    key: &'a str,
    organization_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    activation_id: Option<Uuid>,
}

#[derive(Serialize)]
struct DeactivateRequest<'a> {
    key: &'a str,
    organization_id: Uuid,
    activation_id: Uuid,
}

#[derive(Deserialize)]
struct ActivateResponse {
    id: Uuid,
    label: String,
    license_key: LicenseResponse,
}

#[derive(Deserialize)]
struct LicenseResponse {
    benefit_id: Uuid,
    status: PolarLicenseState,
    expires_at: Option<DateTime<Utc>>,
}

impl From<LicenseResponse> for PolarLicenseResult {
    fn from(value: LicenseResponse) -> Self {
        Self {
            benefit_id: value.benefit_id,
            status: value.status,
            expires_at: value.expires_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::sync::mpsc;
    use tiny_http::{Header, Response as TinyResponse, Server};

    const KEY: &str = "TEST-LICENSE-KEY-SECRET";
    const ACTIVATION: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    const BENEFIT: &str = "11111111-1111-4111-8111-111111111111";

    #[derive(Debug)]
    struct CapturedRequest {
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        body: Value,
    }

    fn mock_config(port: u16) -> PolarConfig {
        PolarConfig::for_test(Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap())
    }

    fn spawn_response(
        status: u16,
        body: String,
        delay: Option<Duration>,
    ) -> (
        PolarConfig,
        mpsc::Receiver<CapturedRequest>,
        std::thread::JoinHandle<()>,
    ) {
        let server = Server::http(("127.0.0.1", 0)).expect("mock server");
        let port = server.server_addr().to_ip().expect("mock address").port();
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut request = server.recv().expect("mock request");
            let mut raw_body = String::new();
            request
                .as_reader()
                .read_to_string(&mut raw_body)
                .expect("read request body");
            let captured = CapturedRequest {
                method: request.method().as_str().to_owned(),
                path: request.url().to_owned(),
                headers: request
                    .headers()
                    .iter()
                    .map(|header| {
                        (
                            header.field.as_str().as_str().to_owned(),
                            header.value.as_str().to_owned(),
                        )
                    })
                    .collect(),
                body: serde_json::from_str(&raw_body).unwrap_or(Value::Null),
            };
            let _ = sender.send(captured);
            if let Some(delay) = delay {
                std::thread::sleep(delay);
            }
            let content_type =
                Header::from_bytes("Content-Type", "application/json").expect("header");
            let _ = request.respond(
                TinyResponse::from_string(body)
                    .with_status_code(status)
                    .with_header(content_type),
            );
        });
        (mock_config(port), receiver, thread)
    }

    fn license_json() -> Value {
        json!({
            "id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
            "created_at": "2026-08-01T00:00:00Z",
            "customer": {"email": "must-not-be-deserialized@example.com"},
            "benefit_id": BENEFIT,
            "key": "raw-response-key-must-be-ignored",
            "display_key": "****-SECRET",
            "status": "granted",
            "expires_at": "2027-08-01T00:00:00Z"
        })
    }

    fn assert_public_request(request: &CapturedRequest, expected_path: &str, expected_body: Value) {
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, expected_path);
        assert_eq!(request.body, expected_body);
        assert!(!request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization")));
    }

    #[tokio::test]
    async fn uses_exact_public_paths_bodies_and_never_authorization() {
        let activate_body = json!({
            "id": ACTIVATION,
            "label": "Test Device",
            "license_key": license_json(),
            "ignored_customer": {"email": "also-ignored@example.com"}
        });
        let (config, captured, thread) = spawn_response(200, activate_body.to_string(), None);
        let client = PolarClient::new(&config).expect("client");
        let result = client.activate(KEY, "Test Device").await.expect("activate");
        assert_eq!(result.activation_id.to_string(), ACTIVATION);
        assert_eq!(result.license.benefit_id.to_string(), BENEFIT);
        assert_public_request(
            &captured.recv().expect("captured activate"),
            "/v1/customer-portal/license-keys/activate",
            json!({
                "key": KEY,
                "organization_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "label": "Test Device"
            }),
        );
        thread.join().expect("activate server");

        let (config, captured, thread) = spawn_response(200, license_json().to_string(), None);
        let client = PolarClient::new(&config).expect("client");
        client
            .validate(KEY, Some(Uuid::parse_str(ACTIVATION).unwrap()))
            .await
            .expect("validate");
        assert_public_request(
            &captured.recv().expect("captured validate"),
            "/v1/customer-portal/license-keys/validate",
            json!({
                "key": KEY,
                "organization_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "activation_id": ACTIVATION
            }),
        );
        thread.join().expect("validate server");

        let (config, captured, thread) = spawn_response(204, String::new(), None);
        let client = PolarClient::new(&config).expect("client");
        client
            .deactivate(KEY, Uuid::parse_str(ACTIVATION).unwrap())
            .await
            .expect("deactivate");
        assert_public_request(
            &captured.recv().expect("captured deactivate"),
            "/v1/customer-portal/license-keys/deactivate",
            json!({
                "key": KEY,
                "organization_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "activation_id": ACTIVATION
            }),
        );
        thread.join().expect("deactivate server");
    }

    #[tokio::test]
    async fn accepts_unknown_fields_but_rejects_malformed_or_oversized_json() {
        let (config, _, thread) = spawn_response(200, license_json().to_string(), None);
        let client = PolarClient::new(&config).expect("client");
        assert_eq!(
            client
                .validate(KEY, Some(Uuid::parse_str(ACTIVATION).unwrap()))
                .await
                .expect("unknown fields")
                .status,
            PolarLicenseState::Granted
        );
        thread.join().expect("unknown-fields server");

        let (config, _, thread) = spawn_response(200, "{not json".into(), None);
        let client = PolarClient::new(&config).expect("client");
        assert_eq!(
            client
                .validate(KEY, Some(Uuid::parse_str(ACTIVATION).unwrap()))
                .await
                .unwrap_err(),
            PolarClientError::InvalidResponse
        );
        thread.join().expect("malformed server");

        let oversized = "x".repeat(MAX_RESPONSE_BYTES + 1);
        let (config, _, thread) = spawn_response(200, oversized, None);
        let client = PolarClient::new(&config).expect("client");
        assert_eq!(
            client
                .validate(KEY, Some(Uuid::parse_str(ACTIVATION).unwrap()))
                .await
                .unwrap_err(),
            PolarClientError::ResponseTooLarge
        );
        thread.join().expect("oversized server");
    }

    #[tokio::test]
    async fn classifies_documented_and_transient_http_statuses() {
        for (status, expected) in [
            (403, PolarClientError::DeviceLimit),
            (404, PolarClientError::InvalidLicense),
            (422, PolarClientError::InvalidLicense),
            (429, PolarClientError::RateLimited),
            (500, PolarClientError::ServiceUnavailable),
            (503, PolarClientError::ServiceUnavailable),
        ] {
            let (config, _, thread) = spawn_response(status, "{}".into(), None);
            let client = PolarClient::new(&config).expect("client");
            assert_eq!(client.activate(KEY, "Device").await.unwrap_err(), expected);
            thread.join().expect("status server");
        }
        assert!(PolarClientError::RateLimited.is_transient());
        assert!(PolarClientError::ServiceUnavailable.is_transient());
        assert!(!PolarClientError::InvalidLicense.is_transient());
    }

    #[tokio::test]
    async fn recognizes_polar_response_for_a_license_without_activations() {
        let (config, captured, thread) = spawn_response(
            403,
            r#"{"detail":"This license key does not support activations. Use the /validate endpoint instead to check license validity."}"#.into(),
            None,
        );
        let client = PolarClient::new(&config).expect("client");

        assert_eq!(
            client.activate(KEY, "Device").await.unwrap_err(),
            PolarClientError::ActivationsUnsupported
        );
        let request = captured.recv().expect("captured request");
        assert_eq!(request.path, "/v1/customer-portal/license-keys/activate");
        thread.join().expect("unsupported activations server");
    }

    #[tokio::test]
    async fn omits_activation_id_when_validating_without_device_activations() {
        let (config, captured, thread) = spawn_response(200, license_json().to_string(), None);
        let client = PolarClient::new(&config).expect("client");

        client.validate(KEY, None).await.expect("validate");
        assert_public_request(
            &captured.recv().expect("captured validate"),
            "/v1/customer-portal/license-keys/validate",
            json!({
                "key": KEY,
                "organization_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
            }),
        );
        thread.join().expect("no activation validation server");
    }

    #[tokio::test]
    async fn request_timeout_is_bounded_and_transient() {
        let (config, _, thread) = spawn_response(
            200,
            license_json().to_string(),
            Some(Duration::from_millis(150)),
        );
        let client = PolarClient::with_timeouts(
            &config,
            Duration::from_millis(30),
            Duration::from_millis(30),
        )
        .expect("client");
        let error = client
            .validate(KEY, Some(Uuid::parse_str(ACTIVATION).unwrap()))
            .await
            .unwrap_err();
        assert_eq!(error, PolarClientError::Timeout);
        assert!(error.is_transient());
        thread.join().expect("timeout server");
    }
}
