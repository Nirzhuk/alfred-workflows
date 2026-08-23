use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::db::{Db, StoredLicenseSnapshot};

use super::client::{
    PolarActivationResult, PolarClient, PolarClientError, PolarLicenseResult, PolarLicenseState,
};
use super::config::PolarConfig;
use super::models::{LicenseCommandError, LicenseProduct, LicenseStatus, LicenseStatusDto};
use super::offline::{
    evaluate_cached_state, state_after_transient_failure, OFFLINE_GRACE_DAYS, REFRESH_AFTER_DAYS,
};
use super::store::{
    LicenseCredentialEnvelope, LicenseCredentialStore, LicenseStoreError, OsLicenseCredentialStore,
};

/// The stable DTO code for a closed update window. It is a fact about the
/// license, not a failure: the customer keeps every feature they paid for.
pub(crate) const UPDATE_WINDOW_CLOSED: &str = "update_window_closed";

pub trait LicenseClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Default)]
pub struct SystemLicenseClock;

impl LicenseClock for SystemLicenseClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub struct LicenseService {
    config: Option<PolarConfig>,
    config_error: Option<String>,
    client: Option<PolarClient>,
    store: Arc<dyn LicenseCredentialStore>,
    clock: Arc<dyn LicenseClock>,
}

impl LicenseService {
    pub fn load() -> Self {
        match PolarConfig::load() {
            Ok(Some(config)) => match PolarClient::new(&config) {
                Ok(client) => Self::configured(
                    config,
                    client,
                    Arc::new(OsLicenseCredentialStore),
                    Arc::new(SystemLicenseClock),
                ),
                Err(error) => Self::unconfigured(Some(error.code())),
            },
            Ok(None) => Self::unconfigured(None),
            Err(error) => Self::unconfigured(Some(error.code())),
        }
    }

    fn unconfigured(error_code: Option<&str>) -> Self {
        Self {
            config: None,
            config_error: error_code.map(str::to_owned),
            client: None,
            store: Arc::new(OsLicenseCredentialStore),
            clock: Arc::new(SystemLicenseClock),
        }
    }

    pub(crate) fn configured(
        config: PolarConfig,
        client: PolarClient,
        store: Arc<dyn LicenseCredentialStore>,
        clock: Arc<dyn LicenseClock>,
    ) -> Self {
        Self {
            config: Some(config),
            config_error: None,
            client: Some(client),
            store,
            clock,
        }
    }

    pub fn get_status(&self, db: &Db) -> Result<LicenseStatusDto, LicenseCommandError> {
        if self.config.is_none() {
            return Ok(LicenseStatusDto::not_configured(
                self.config_error.as_deref(),
            ));
        }

        let Some(snapshot) = db.get_license_snapshot().map_err(|_| local_state_error())? else {
            return Ok(LicenseStatusDto::unlicensed());
        };
        if snapshot.credential_ref.is_none() {
            db.delete_license_snapshot()
                .map_err(|_| local_state_error())?;
            return Ok(LicenseStatusDto::unlicensed());
        }
        let mut dto = LicenseStatusDto::from_stored(&snapshot).map_err(|_| local_state_error())?;
        let evaluation = evaluate_cached_state(
            dto.state,
            parse_time(dto.update_deadline.as_deref()),
            parse_time(dto.next_refresh.as_deref()),
            parse_time(dto.offline_deadline.as_deref()),
            self.clock.now(),
        );
        dto.state = evaluation.state;
        if dto.state == LicenseStatus::Expired {
            dto.error_code = Some(UPDATE_WINDOW_CLOSED.into());
        } else if dto.state == LicenseStatus::NeedsOnline {
            dto.error_code = Some("online_validation_required".into());
        } else if matches!(
            dto.state,
            LicenseStatus::Active | LicenseStatus::OfflineGrace
        ) && dto.error_code.as_deref() == Some(UPDATE_WINDOW_CLOSED)
        {
            dto.error_code = None;
        }
        Ok(dto)
    }

    pub fn should_refresh(&self, db: &Db) -> bool {
        let Ok(Some(snapshot)) = db.get_license_snapshot() else {
            return false;
        };
        if self.config.is_none() || snapshot.credential_ref.is_none() {
            return false;
        }
        let Ok(dto) = LicenseStatusDto::from_stored(&snapshot) else {
            return false;
        };
        evaluate_cached_state(
            dto.state,
            parse_time(dto.update_deadline.as_deref()),
            parse_time(dto.next_refresh.as_deref()),
            parse_time(dto.offline_deadline.as_deref()),
            self.clock.now(),
        )
        .should_refresh
    }

    pub async fn activate(
        &self,
        db: &Db,
        license_key: String,
        device_label: String,
    ) -> Result<LicenseStatusDto, LicenseCommandError> {
        // Wrap the Tauri-owned input before any early return or await.
        let license_key = Zeroizing::new(license_key);
        let Some((config, client)) = self.config_and_client() else {
            return Ok(LicenseStatusDto::not_configured(
                self.config_error.as_deref(),
            ));
        };
        let existing_snapshot = db.get_license_snapshot().map_err(|_| local_state_error())?;
        if existing_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.credential_ref.is_some())
        {
            return Err(LicenseCommandError::new("license_already_active", true));
        }
        if existing_snapshot.is_some() {
            db.delete_license_snapshot()
                .map_err(|_| local_state_error())?;
        }

        let device_label = device_label.trim();
        if license_key.is_empty()
            || license_key.len() > 256
            || license_key.trim().len() != license_key.len()
            || device_label.is_empty()
            || device_label.chars().count() > 120
        {
            return Err(LicenseCommandError::new("invalid_input", true));
        }
        let masked_key = mask_key(&license_key);
        let now = self.clock.now();
        let activation = match client.activate(&license_key, device_label).await {
            Ok(activation) => activation,
            Err(PolarClientError::ActivationsUnsupported) => {
                let validated = match client.validate(&license_key, None).await {
                    Ok(result) => result,
                    Err(error) => {
                        let dto = activation_failure_status(masked_key, error);
                        return Ok(dto);
                    }
                };
                let Some(product) = config.product_for_benefit(&validated.benefit_id.to_string())
                else {
                    let dto = failure_status(
                        masked_key,
                        LicenseStatus::Unlicensed,
                        "unsupported_product",
                    );
                    return Ok(dto);
                };
                let state = confirmed_state(&validated, now);
                if state != LicenseStatus::Active {
                    let dto = confirmed_failure_status(masked_key, product, &validated, state);
                    return Ok(dto);
                }

                return self
                    .persist_license(
                        client,
                        license_key,
                        None,
                        product,
                        validated,
                        None,
                        now,
                        |snapshot| db.put_license_snapshot(snapshot).map_err(|_| ()),
                    )
                    .await;
            }
            Err(error) => {
                let dto = activation_failure_status(masked_key, error);
                return Ok(dto);
            }
        };

        let Some(activation_product) =
            config.product_for_benefit(&activation.license.benefit_id.to_string())
        else {
            let _ = client
                .deactivate(&license_key, activation.activation_id)
                .await;
            let dto = failure_status(masked_key, LicenseStatus::Unlicensed, "unsupported_product");
            return Ok(dto);
        };

        let state = confirmed_state(&activation.license, now);
        if state != LicenseStatus::Active {
            let _ = client
                .deactivate(&license_key, activation.activation_id)
                .await;
            let dto = confirmed_failure_status(
                masked_key,
                activation_product,
                &activation.license,
                state,
            );
            return Ok(dto);
        }

        let validated = match client
            .validate(&license_key, Some(activation.activation_id))
            .await
        {
            Ok(result) => result,
            Err(error) => {
                let _ = client
                    .deactivate(&license_key, activation.activation_id)
                    .await;
                let dto = activation_failure_status(masked_key, error);
                return Ok(dto);
            }
        };

        let Some(product) = config.product_for_benefit(&validated.benefit_id.to_string()) else {
            let _ = client
                .deactivate(&license_key, activation.activation_id)
                .await;
            let dto = failure_status(masked_key, LicenseStatus::Unlicensed, "unsupported_product");
            return Ok(dto);
        };

        let state = confirmed_state(&validated, now);
        if state != LicenseStatus::Active {
            let _ = client
                .deactivate(&license_key, activation.activation_id)
                .await;
            let dto = confirmed_failure_status(masked_key, product, &validated, state);
            return Ok(dto);
        }

        let validated_activation = PolarActivationResult {
            activation_id: activation.activation_id,
            label: activation.label,
            license: validated,
        };

        self.persist_activation(
            client,
            license_key,
            device_label,
            product,
            validated_activation,
            now,
            |snapshot| db.put_license_snapshot(snapshot).map_err(|_| ()),
        )
        .await
    }

    async fn persist_activation<F>(
        &self,
        client: &PolarClient,
        license_key: Zeroizing<String>,
        device_label: &str,
        product: LicenseProduct,
        activation: PolarActivationResult,
        now: DateTime<Utc>,
        persist_snapshot: F,
    ) -> Result<LicenseStatusDto, LicenseCommandError>
    where
        F: FnOnce(&StoredLicenseSnapshot) -> Result<(), ()>,
    {
        self.persist_license(
            client,
            license_key,
            Some(device_label),
            product,
            activation.license,
            Some(activation.activation_id),
            now,
            persist_snapshot,
        )
        .await
    }

    async fn persist_license<F>(
        &self,
        client: &PolarClient,
        mut license_key: Zeroizing<String>,
        device_label: Option<&str>,
        product: LicenseProduct,
        license: PolarLicenseResult,
        activation_id: Option<Uuid>,
        now: DateTime<Utc>,
        persist_snapshot: F,
    ) -> Result<LicenseStatusDto, LicenseCommandError>
    where
        F: FnOnce(&StoredLicenseSnapshot) -> Result<(), ()>,
    {
        let masked_key = mask_key(&license_key);
        let credential_ref = format!("license-{}", Uuid::new_v4());
        let envelope = match activation_id {
            Some(activation_id) => LicenseCredentialEnvelope::new(
                std::mem::take(&mut *license_key),
                activation_id.to_string(),
            ),
            None => {
                LicenseCredentialEnvelope::without_activation(std::mem::take(&mut *license_key))
            }
        };
        if self.store.put(&credential_ref, &envelope).is_err() {
            self.compensate_activation(client, &credential_ref, &envelope, activation_id)
                .await;
            let dto = failure_status(
                masked_key,
                LicenseStatus::SecureStorageUnavailable,
                "secure_storage_unavailable",
            );
            return Ok(dto);
        }

        let mut dto = LicenseStatusDto {
            product,
            state: LicenseStatus::Active,
            masked_key: Some(masked_key),
            benefit_id: Some(license.benefit_id.to_string()),
            // Use the locally validated input, never customer-bearing response data.
            activation_label: device_label.map(str::to_owned),
            current_device: activation_id.is_some(),
            last_successful_validation: Some(now.to_rfc3339()),
            next_refresh: Some((now + Duration::days(REFRESH_AFTER_DAYS)).to_rfc3339()),
            offline_deadline: Some((now + Duration::days(OFFLINE_GRACE_DAYS)).to_rfc3339()),
            error_code: None,
            ..LicenseStatusDto::unlicensed()
        };
        dto.set_update_deadline(license.expires_at.map(|time| time.to_rfc3339()));
        if persist_snapshot(&dto.clone().into_stored(Some(credential_ref.clone()), now)).is_err() {
            self.compensate_activation(client, &credential_ref, &envelope, activation_id)
                .await;
            return Err(local_state_error());
        }
        Ok(dto)
    }

    async fn compensate_activation(
        &self,
        client: &PolarClient,
        credential_ref: &str,
        envelope: &LicenseCredentialEnvelope,
        activation_id: Option<Uuid>,
    ) {
        // `put` may fail after partially writing, so cleanup is attempted even
        // on its error path. The in-memory envelope remains zeroizing and is
        // sufficient for the best-effort remote rollback.
        let _ = self.store.delete(credential_ref);
        if let Some(activation_id) = activation_id {
            let _ = client
                .deactivate(&envelope.license_key, activation_id)
                .await;
        }
    }

    pub async fn refresh(&self, db: &Db) -> Result<LicenseStatusDto, LicenseCommandError> {
        let Some((config, client)) = self.config_and_client() else {
            return Ok(LicenseStatusDto::not_configured(
                self.config_error.as_deref(),
            ));
        };
        let Some(snapshot) = db.get_license_snapshot().map_err(|_| local_state_error())? else {
            return Ok(LicenseStatusDto::unlicensed());
        };
        let Some(credential_ref) = snapshot.credential_ref.clone() else {
            db.delete_license_snapshot()
                .map_err(|_| local_state_error())?;
            return Ok(LicenseStatusDto::unlicensed());
        };
        let credential = match self.store.get(&credential_ref) {
            Ok(credential) => credential,
            Err(_) => {
                return self.persist_storage_failure(db, snapshot, "secure_storage_unavailable")
            }
        };
        let activation_id = match credential.activation_id.as_deref() {
            Some(value) => match Uuid::parse_str(value) {
                Ok(id) => Some(id),
                Err(_) => {
                    return self.persist_storage_failure(db, snapshot, "secure_storage_invalid")
                }
            },
            None => None,
        };
        let now = self.clock.now();

        match client
            .validate(&credential.license_key, activation_id)
            .await
        {
            Ok(result) => {
                let product = config.product_for_benefit(&result.benefit_id.to_string());
                self.persist_validation(db, snapshot, product, result, now)
            }
            Err(error) => self.persist_validation_error(db, snapshot, error, now),
        }
    }

    fn persist_validation(
        &self,
        db: &Db,
        snapshot: StoredLicenseSnapshot,
        product: Option<LicenseProduct>,
        result: PolarLicenseResult,
        now: DateTime<Utc>,
    ) -> Result<LicenseStatusDto, LicenseCommandError> {
        let mut dto = LicenseStatusDto::from_stored(&snapshot).map_err(|_| local_state_error())?;
        dto.benefit_id = Some(result.benefit_id.to_string());
        dto.set_update_deadline(result.expires_at.map(|time| time.to_rfc3339()));

        if let Some(product) = product {
            dto.product = product;
            dto.state = confirmed_state(&result, now);
            dto.error_code = match dto.state {
                LicenseStatus::Expired => Some(UPDATE_WINDOW_CLOSED.into()),
                LicenseStatus::Revoked => Some("license_revoked".into()),
                LicenseStatus::Disabled => Some("license_disabled".into()),
                _ => None,
            };
        } else {
            dto.state = LicenseStatus::Disabled;
            dto.error_code = Some("unsupported_product".into());
        }

        if dto.state == LicenseStatus::Active {
            dto.last_successful_validation = Some(now.to_rfc3339());
            dto.next_refresh = Some((now + Duration::days(REFRESH_AFTER_DAYS)).to_rfc3339());
            dto.offline_deadline = Some((now + Duration::days(OFFLINE_GRACE_DAYS)).to_rfc3339());
        }
        db.put_license_snapshot(
            &dto.clone()
                .into_stored(snapshot.credential_ref.clone(), now),
        )
        .map_err(|_| local_state_error())?;
        Ok(dto)
    }

    fn persist_validation_error(
        &self,
        db: &Db,
        snapshot: StoredLicenseSnapshot,
        error: PolarClientError,
        now: DateTime<Utc>,
    ) -> Result<LicenseStatusDto, LicenseCommandError> {
        let mut dto = LicenseStatusDto::from_stored(&snapshot).map_err(|_| local_state_error())?;
        dto.error_code = Some(error.code().into());
        let prior_state = dto.state;
        dto.state = if error.is_transient() {
            state_after_transient_failure(
                prior_state,
                parse_time(dto.offline_deadline.as_deref()),
                now,
            )
        } else {
            match error {
                PolarClientError::InvalidLicense => LicenseStatus::Revoked,
                PolarClientError::DeviceLimit => LicenseStatus::DeviceLimit,
                PolarClientError::ResponseTooLarge | PolarClientError::InvalidResponse => {
                    LicenseStatus::NeedsOnline
                }
                _ => prior_state,
            }
        };
        db.put_license_snapshot(
            &dto.clone()
                .into_stored(snapshot.credential_ref.clone(), now),
        )
        .map_err(|_| local_state_error())?;
        Ok(dto)
    }

    fn persist_storage_failure(
        &self,
        db: &Db,
        snapshot: StoredLicenseSnapshot,
        code: &str,
    ) -> Result<LicenseStatusDto, LicenseCommandError> {
        let now = self.clock.now();
        let mut dto = LicenseStatusDto::from_stored(&snapshot).map_err(|_| local_state_error())?;
        dto.state = LicenseStatus::SecureStorageUnavailable;
        dto.error_code = Some(code.into());
        db.put_license_snapshot(&dto.clone().into_stored(snapshot.credential_ref, now))
            .map_err(|_| local_state_error())?;
        Ok(dto)
    }

    pub async fn deactivate(&self, db: &Db) -> Result<LicenseStatusDto, LicenseCommandError> {
        let Some((_, client)) = self.config_and_client() else {
            return Err(LicenseCommandError::new("polar_not_configured", true));
        };
        let Some(snapshot) = db.get_license_snapshot().map_err(|_| local_state_error())? else {
            return Ok(LicenseStatusDto::unlicensed());
        };
        let Some(credential_ref) = snapshot.credential_ref.clone() else {
            db.delete_license_snapshot()
                .map_err(|_| local_state_error())?;
            return Ok(LicenseStatusDto::unlicensed());
        };
        let credential = self.store.get(&credential_ref).map_err(map_store_error)?;
        let Some(activation_id) = credential.activation_id.as_deref() else {
            if self.store.delete(&credential_ref).is_err() {
                return self.persist_storage_failure(db, snapshot, "secure_storage_unavailable");
            }
            db.delete_license_snapshot()
                .map_err(|_| local_state_error())?;
            return Ok(LicenseStatusDto::unlicensed());
        };
        let activation_id = Uuid::parse_str(activation_id)
            .map_err(|_| LicenseCommandError::new("secure_storage_invalid", false))?;

        if let Err(error) = client
            .deactivate(&credential.license_key, activation_id)
            .await
        {
            // No local mutation: a failed remote deactivation must be retryable.
            return Err(LicenseCommandError::new(error.code(), error.is_transient()));
        }
        if self.store.delete(&credential_ref).is_err() {
            return self.persist_storage_failure(db, snapshot, "secure_storage_unavailable");
        }
        db.delete_license_snapshot()
            .map_err(|_| local_state_error())?;
        Ok(LicenseStatusDto::unlicensed())
    }

    fn config_and_client(&self) -> Option<(&PolarConfig, &PolarClient)> {
        self.config.as_ref().zip(self.client.as_ref())
    }
}

fn activation_failure_status(masked_key: String, error: PolarClientError) -> LicenseStatusDto {
    match error {
        PolarClientError::DeviceLimit => {
            failure_status(masked_key, LicenseStatus::DeviceLimit, "device_limit")
        }
        _ => failure_status(masked_key, LicenseStatus::Unlicensed, error.code()),
    }
}

fn confirmed_failure_status(
    masked_key: String,
    product: LicenseProduct,
    result: &PolarLicenseResult,
    state: LicenseStatus,
) -> LicenseStatusDto {
    let mut dto = LicenseStatusDto {
        product,
        state,
        masked_key: Some(masked_key),
        benefit_id: Some(result.benefit_id.to_string()),
        error_code: Some(
            match state {
                LicenseStatus::Expired => UPDATE_WINDOW_CLOSED,
                LicenseStatus::Revoked => "license_revoked",
                LicenseStatus::Disabled => "license_disabled",
                _ => "license_invalid",
            }
            .into(),
        ),
        ..LicenseStatusDto::unlicensed()
    };
    dto.set_update_deadline(result.expires_at.map(|time| time.to_rfc3339()));
    dto
}

fn failure_status(masked_key: String, state: LicenseStatus, code: &str) -> LicenseStatusDto {
    LicenseStatusDto {
        state,
        masked_key: Some(masked_key),
        error_code: Some(code.into()),
        ..LicenseStatusDto::unlicensed()
    }
}

/// Both products are perpetual, so Polar's expiry date is the end of the
/// included update window, never the end of the license. A key past it is
/// `Expired`: still entitled, window closed. `Revoked` and `Disabled` are the
/// two answers that do end entitlement, so they are read first.
fn confirmed_state(result: &PolarLicenseResult, now: DateTime<Utc>) -> LicenseStatus {
    match result.status {
        PolarLicenseState::Revoked => LicenseStatus::Revoked,
        PolarLicenseState::Disabled => LicenseStatus::Disabled,
        PolarLicenseState::Granted
            if result.expires_at.is_some_and(|expires| now >= expires) =>
        {
            LicenseStatus::Expired
        }
        PolarLicenseState::Granted => LicenseStatus::Active,
    }
}

fn mask_key(key: &str) -> String {
    let suffix: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("••••-{suffix}")
}

fn parse_time(value: Option<&str>) -> Option<DateTime<Utc>> {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn local_state_error() -> LicenseCommandError {
    LicenseCommandError::new("license_state_unavailable", true)
}

fn map_store_error(error: LicenseStoreError) -> LicenseCommandError {
    match error {
        LicenseStoreError::Locked => LicenseCommandError::new("secure_storage_unavailable", true),
        LicenseStoreError::Missing | LicenseStoreError::Invalid | LicenseStoreError::Failed => {
            LicenseCommandError::new("secure_storage_invalid", false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::licensing::store::InMemoryLicenseCredentialStore;
    use chrono::TimeZone;
    use serde_json::{json, Value};
    use std::sync::{mpsc, Mutex};
    use std::time::Duration as StdDuration;
    use tiny_http::{Header, Response, Server};

    const KEY: &str = "TEST-LICENSE-KEY-SECRET";
    const ACTIVATION: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    const BENEFIT: &str = "11111111-1111-4111-8111-111111111111";

    #[derive(Debug)]
    struct FixedClock(Mutex<DateTime<Utc>>);

    impl FixedClock {
        fn at(time: DateTime<Utc>) -> Self {
            Self(Mutex::new(time))
        }
    }

    impl LicenseClock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            *self.0.lock().expect("clock")
        }
    }

    #[derive(Debug, Default)]
    struct FailAfterWriteStore {
        inner: InMemoryLicenseCredentialStore,
    }

    impl LicenseCredentialStore for FailAfterWriteStore {
        fn put(
            &self,
            credential_ref: &str,
            credential: &LicenseCredentialEnvelope,
        ) -> Result<(), LicenseStoreError> {
            self.inner.put(credential_ref, credential)?;
            Err(LicenseStoreError::Failed)
        }
        fn get(
            &self,
            credential_ref: &str,
        ) -> Result<LicenseCredentialEnvelope, LicenseStoreError> {
            self.inner.get(credential_ref)
        }
        fn delete(&self, credential_ref: &str) -> Result<(), LicenseStoreError> {
            self.inner.delete(credential_ref)
        }
    }

    #[derive(Debug)]
    struct CapturedRequest {
        path: String,
        body: Value,
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap()
    }

    fn activation_body(status: &str, benefit: &str, expires_at: Option<&str>) -> Value {
        json!({
            "id": ACTIVATION,
            "label": "ignored server label",
            "customer": {"email": "ignored@example.com"},
            "license_key": {
                "benefit_id": benefit,
                "status": status,
                "expires_at": expires_at,
                "key": "raw-response-secret"
            }
        })
    }

    fn validation_body(status: &str, benefit: &str, expires_at: Option<&str>) -> Value {
        json!({
            "benefit_id": benefit,
            "status": status,
            "expires_at": expires_at,
            "customer": {"email": "ignored@example.com"},
            "key": "raw-response-secret"
        })
    }

    fn spawn_capturing_server(
        responses: Vec<(u16, String)>,
    ) -> (
        PolarConfig,
        mpsc::Receiver<CapturedRequest>,
        std::thread::JoinHandle<()>,
    ) {
        let server = Server::http(("127.0.0.1", 0)).expect("mock server");
        let port = server.server_addr().to_ip().expect("mock address").port();
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            for (status, body) in responses {
                let mut request = server.recv().expect("request");
                let path = request.url().to_owned();
                let mut request_body = String::new();
                request
                    .as_reader()
                    .read_to_string(&mut request_body)
                    .expect("request body");
                let _ = sender.send(CapturedRequest {
                    path,
                    body: serde_json::from_str(&request_body).unwrap_or(Value::Null),
                });
                let content_type =
                    Header::from_bytes("Content-Type", "application/json").expect("header");
                request
                    .respond(
                        Response::from_string(body)
                            .with_status_code(status)
                            .with_header(content_type),
                    )
                    .expect("response");
            }
        });
        (
            PolarConfig::for_test(url::Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap()),
            receiver,
            thread,
        )
    }

    fn spawn_server(responses: Vec<(u16, String)>) -> (PolarConfig, std::thread::JoinHandle<()>) {
        let (config, _captured, thread) = spawn_capturing_server(responses);
        (config, thread)
    }

    fn build_service(
        config: PolarConfig,
        store: Arc<dyn LicenseCredentialStore>,
        time: DateTime<Utc>,
    ) -> LicenseService {
        let client = PolarClient::new(&config).expect("client");
        LicenseService::configured(config, client, store, Arc::new(FixedClock::at(time)))
    }

    fn seed_active(db: &Db, store: &dyn LicenseCredentialStore, time: DateTime<Utc>) {
        store
            .put(
                "credential",
                &LicenseCredentialEnvelope::new(KEY.into(), ACTIVATION.into()),
            )
            .expect("seed credential");
        db.put_license_snapshot(&StoredLicenseSnapshot {
            product: "desktopAnnual".into(),
            status: "active".into(),
            masked_key: Some("••••-CRET".into()),
            benefit_id: Some(BENEFIT.into()),
            activation_label: Some("Test Device".into()),
            current_device: true,
            expires_at: None,
            last_success_at: Some(time.to_rfc3339()),
            refresh_due_at: Some((time + Duration::days(7)).to_rfc3339()),
            offline_deadline: Some((time + Duration::days(30)).to_rfc3339()),
            error_code: None,
            credential_ref: Some("credential".into()),
            updated_at: time.to_rfc3339(),
        })
        .expect("seed snapshot");
    }

    #[tokio::test]
    async fn activation_stores_secrets_only_in_credential_store_and_returns_safe_dto() {
        let (config, captured, thread) = spawn_capturing_server(vec![
            (
                200,
                activation_body("granted", BENEFIT, Some("2027-08-15T12:00:00Z")).to_string(),
            ),
            (
                200,
                validation_body("granted", BENEFIT, Some("2027-08-15T12:00:00Z")).to_string(),
            ),
        ]);
        let store = Arc::new(InMemoryLicenseCredentialStore::default());
        let service = build_service(config, store.clone(), now());
        let db = Db::open_in_memory().expect("database");

        let status = service
            .activate(&db, KEY.into(), " Test Device ".into())
            .await
            .expect("activate");
        assert_eq!(
            captured.recv().expect("captured activate").path,
            "/v1/customer-portal/license-keys/activate"
        );
        let validate_request = captured.recv().expect("captured validate");
        assert_eq!(
            validate_request.path,
            "/v1/customer-portal/license-keys/validate"
        );
        assert_eq!(
            validate_request.body,
            json!({
                "key": KEY,
                "organization_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "activation_id": ACTIVATION
            })
        );
        thread.join().expect("server");
        assert_eq!(status.state, LicenseStatus::Active);
        assert_eq!(status.product, LicenseProduct::Individual);
        assert_eq!(status.activation_label.as_deref(), Some("Test Device"));
        let serialized = serde_json::to_string(&status).expect("status JSON");
        assert!(!serialized.contains(KEY));
        assert!(!serialized.contains(ACTIVATION));
        assert!(!serialized.contains("credential"));

        let snapshot = db.get_license_snapshot().expect("snapshot").unwrap();
        let envelope = store
            .get(snapshot.credential_ref.as_deref().unwrap())
            .expect("credential");
        assert_eq!(envelope.license_key, KEY);
        assert_eq!(envelope.activation_id.as_deref(), Some(ACTIVATION));
        db.with_conn(|conn| {
            let values: String = conn.query_row(
                "SELECT coalesce(product,'') || coalesce(status,'') || coalesce(masked_key,'') ||
                        coalesce(benefit_id,'') || coalesce(activation_label,'') ||
                        coalesce(expires_at,'') || coalesce(last_success_at,'') ||
                        coalesce(refresh_due_at,'') || coalesce(offline_deadline,'') ||
                        coalesce(error_code,'') || coalesce(credential_ref,'')
                   FROM license_snapshot",
                [],
                |row| row.get(0),
            )?;
            assert!(!values.contains(KEY));
            assert!(!values.contains(ACTIVATION));
            Ok(())
        })
        .expect("inspect safe snapshot");
    }

    #[tokio::test]
    async fn activation_falls_back_to_direct_validation_without_device_activations() {
        let (config, captured, thread) = spawn_capturing_server(vec![
            (
                403,
                r#"{"detail":"This license key does not support activations. Use the /validate endpoint instead to check license validity."}"#.into(),
            ),
            (
                200,
                validation_body("granted", BENEFIT, Some("2027-08-15T12:00:00Z")).to_string(),
            ),
            (
                200,
                validation_body("granted", BENEFIT, Some("2027-08-15T12:00:00Z")).to_string(),
            ),
        ]);
        let store = Arc::new(InMemoryLicenseCredentialStore::default());
        let service = build_service(config, store.clone(), now());
        let db = Db::open_in_memory().expect("database");

        let status = service
            .activate(&db, KEY.into(), "Test Device".into())
            .await
            .expect("activate without device activations");
        assert_eq!(status.state, LicenseStatus::Active);
        assert_eq!(status.product, LicenseProduct::Individual);
        assert!(!status.current_device);
        assert_eq!(status.activation_label, None);

        assert_eq!(
            captured.recv().expect("captured activate").path,
            "/v1/customer-portal/license-keys/activate"
        );
        let validate_request = captured.recv().expect("captured direct validate");
        assert_eq!(
            validate_request.path,
            "/v1/customer-portal/license-keys/validate"
        );
        assert_eq!(
            validate_request.body,
            json!({
                "key": KEY,
                "organization_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
            })
        );

        let envelope = store
            .get(
                db.get_license_snapshot()
                    .expect("snapshot")
                    .expect("stored snapshot")
                    .credential_ref
                    .as_deref()
                    .expect("credential reference"),
            )
            .expect("credential");
        assert_eq!(envelope.activation_id, None);

        let refreshed = service.refresh(&db).await.expect("refresh");
        assert_eq!(refreshed.state, LicenseStatus::Active);
        let refresh_request = captured.recv().expect("captured refresh validate");
        assert_eq!(
            refresh_request.body,
            json!({
                "key": KEY,
                "organization_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
            })
        );
        thread.join().expect("direct validation server");
    }

    #[tokio::test]
    async fn activation_rejects_unknown_non_granted_and_device_limit() {
        for (response, expected_state, expected_code, requests) in [
            (
                (
                    200,
                    activation_body("granted", "44444444-4444-4444-8444-444444444444", None)
                        .to_string(),
                ),
                LicenseStatus::Unlicensed,
                "unsupported_product",
                2,
            ),
            (
                (200, activation_body("revoked", BENEFIT, None).to_string()),
                LicenseStatus::Revoked,
                "license_revoked",
                2,
            ),
            (
                (403, "{}".into()),
                LicenseStatus::DeviceLimit,
                "device_limit",
                1,
            ),
        ] {
            let mut responses = vec![response];
            if requests == 2 {
                responses.push((204, String::new()));
            }
            let (config, thread) = spawn_server(responses);
            let service = build_service(
                config,
                Arc::new(InMemoryLicenseCredentialStore::default()),
                now(),
            );
            let db = Db::open_in_memory().expect("database");
            let status = service
                .activate(&db, KEY.into(), "Device".into())
                .await
                .expect("activation outcome");
            assert_eq!(status.state, expected_state);
            assert_eq!(status.error_code.as_deref(), Some(expected_code));
            assert!(db.get_license_snapshot().expect("snapshot").is_none());
            thread.join().expect("server");
        }
    }

    #[tokio::test]
    async fn secure_store_failure_cleans_partial_write_and_compensates_activation() {
        let (config, captured, thread) = spawn_capturing_server(vec![
            (200, activation_body("granted", BENEFIT, None).to_string()),
            (200, validation_body("granted", BENEFIT, None).to_string()),
            (204, String::new()),
        ]);
        let store = Arc::new(FailAfterWriteStore::default());
        let service = build_service(config, store.clone(), now());
        let db = Db::open_in_memory().expect("database");
        let status = service
            .activate(&db, KEY.into(), "Device".into())
            .await
            .expect("failed-store outcome");
        assert_eq!(status.state, LicenseStatus::SecureStorageUnavailable);
        assert_eq!(
            status.error_code.as_deref(),
            Some("secure_storage_unavailable")
        );
        assert!(store.inner.is_empty());
        let activate_request = captured.recv().expect("captured activate");
        assert_eq!(
            activate_request.path,
            "/v1/customer-portal/license-keys/activate"
        );
        let validate_request = captured.recv().expect("captured validate");
        assert_eq!(
            validate_request.path,
            "/v1/customer-portal/license-keys/validate"
        );
        let deactivate_request = captured.recv().expect("captured compensation");
        assert_eq!(
            deactivate_request.path,
            "/v1/customer-portal/license-keys/deactivate"
        );
        assert_eq!(
            deactivate_request.body,
            json!({
                "key": KEY,
                "organization_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "activation_id": ACTIVATION
            })
        );
        thread.join().expect("server");
    }

    #[tokio::test]
    async fn snapshot_write_failure_cleans_credential_and_compensates_activation() {
        let (config, captured, thread) = spawn_capturing_server(vec![(204, String::new())]);
        let client = PolarClient::new(&config).expect("client");
        let store = Arc::new(InMemoryLicenseCredentialStore::default());
        let service = LicenseService::configured(
            config,
            client.clone(),
            store.clone(),
            Arc::new(FixedClock::at(now())),
        );
        let activation = PolarActivationResult {
            activation_id: Uuid::parse_str(ACTIVATION).unwrap(),
            label: "ignored server label".into(),
            license: PolarLicenseResult {
                benefit_id: Uuid::parse_str(BENEFIT).unwrap(),
                status: PolarLicenseState::Granted,
                expires_at: None,
            },
        };

        let error = service
            .persist_activation(
                &client,
                Zeroizing::new(KEY.into()),
                "Device",
                LicenseProduct::Individual,
                activation,
                now(),
                |_| Err(()),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "license_state_unavailable");
        assert!(store.is_empty());
        let request = captured.recv().expect("captured compensation");
        assert_eq!(request.path, "/v1/customer-portal/license-keys/deactivate");
        assert_eq!(
            request.body,
            json!({
                "key": KEY,
                "organization_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "activation_id": ACTIVATION
            })
        );
        thread.join().expect("server");
    }

    #[test]
    fn cached_status_reads_do_not_contact_polar() {
        let server = Server::http(("127.0.0.1", 0)).expect("mock server");
        let port = server.server_addr().to_ip().expect("mock address").port();
        let config =
            PolarConfig::for_test(url::Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap());
        let store = Arc::new(InMemoryLicenseCredentialStore::default());
        let db = Db::open_in_memory().expect("database");
        seed_active(&db, store.as_ref(), now());
        let service = build_service(config, store, now());

        let status = service.get_status(&db).expect("cached status");

        assert_eq!(status.state, LicenseStatus::Active);
        assert!(server
            .recv_timeout(StdDuration::from_millis(75))
            .expect("inspect mock server")
            .is_none());
    }

    #[test]
    fn uncredentialed_failure_snapshot_is_cleared_on_status_read() {
        let config = PolarConfig::for_test(url::Url::parse("http://127.0.0.1:1/").unwrap());
        let service = build_service(
            config,
            Arc::new(InMemoryLicenseCredentialStore::default()),
            now(),
        );
        let db = Db::open_in_memory().expect("database");
        let failure = failure_status(
            "••••-SAFE".into(),
            LicenseStatus::DeviceLimit,
            "device_limit",
        );
        db.put_license_snapshot(&failure.into_stored(None, now()))
            .expect("failure snapshot");

        let status = service.get_status(&db).expect("status");

        assert_eq!(status, LicenseStatusDto::unlicensed());
        assert!(db
            .get_license_snapshot()
            .expect("cleared snapshot")
            .is_none());
    }

    #[tokio::test]
    async fn validation_applies_confirmed_states_and_a_closed_update_window() {
        for (body, expected_state, expected_code) in [
            (
                validation_body("granted", BENEFIT, None),
                LicenseStatus::Active,
                None,
            ),
            (
                validation_body("revoked", BENEFIT, None),
                LicenseStatus::Revoked,
                Some("license_revoked"),
            ),
            (
                validation_body("disabled", BENEFIT, None),
                LicenseStatus::Disabled,
                Some("license_disabled"),
            ),
            // A passed deadline closes the update window for both products.
            // The key still proves the purchase, so the state stays entitled.
            (
                validation_body("granted", BENEFIT, Some("2026-08-15T11:59:59Z")),
                LicenseStatus::Expired,
                Some(UPDATE_WINDOW_CLOSED),
            ),
            (
                validation_body(
                    "granted",
                    "33333333-3333-4333-8333-333333333333",
                    Some("2026-08-15T11:59:59Z"),
                ),
                LicenseStatus::Expired,
                Some(UPDATE_WINDOW_CLOSED),
            ),
            // A deadline still ahead keeps the window open.
            (
                validation_body("granted", BENEFIT, Some("2027-08-15T11:59:59Z")),
                LicenseStatus::Active,
                None,
            ),
            (
                validation_body("granted", "44444444-4444-4444-8444-444444444444", None),
                LicenseStatus::Disabled,
                Some("unsupported_product"),
            ),
        ] {
            let (config, thread) = spawn_server(vec![(200, body.to_string())]);
            let store = Arc::new(InMemoryLicenseCredentialStore::default());
            let db = Db::open_in_memory().expect("database");
            seed_active(&db, store.as_ref(), now() - Duration::days(8));
            let service = build_service(config, store, now());
            let status = service.refresh(&db).await.expect("refresh");
            assert_eq!(status.state, expected_state);
            assert_eq!(status.error_code.as_deref(), expected_code);
            assert_eq!(
                status.state.is_entitled(),
                !matches!(
                    expected_state,
                    LicenseStatus::Revoked | LicenseStatus::Disabled
                ),
                "{expected_state:?} entitlement"
            );
            thread.join().expect("server");
        }
    }

    #[tokio::test]
    async fn transient_validation_only_grants_bounded_grace_to_prior_grant() {
        let prior = now() - Duration::days(8);
        let (config, thread) = spawn_server(vec![(500, "{}".into())]);
        let store = Arc::new(InMemoryLicenseCredentialStore::default());
        let db = Db::open_in_memory().expect("database");
        seed_active(&db, store.as_ref(), prior);
        let service = build_service(config, store, now());
        let status = service.refresh(&db).await.expect("transient refresh");
        assert_eq!(status.state, LicenseStatus::OfflineGrace);
        assert_eq!(status.error_code.as_deref(), Some("polar_unavailable"));
        thread.join().expect("server");

        let (config, thread) = spawn_server(vec![(500, "{}".into())]);
        let store = Arc::new(InMemoryLicenseCredentialStore::default());
        let db = Db::open_in_memory().expect("database");
        seed_active(&db, store.as_ref(), now() - Duration::days(31));
        let service = build_service(config, store, now());
        assert_eq!(
            service.refresh(&db).await.expect("past grace").state,
            LicenseStatus::NeedsOnline
        );
        thread.join().expect("server");
    }

    #[tokio::test]
    async fn deactivation_clears_only_after_confirmed_remote_success() {
        let (config, thread) = spawn_server(vec![(500, "{}".into())]);
        let store = Arc::new(InMemoryLicenseCredentialStore::default());
        let db = Db::open_in_memory().expect("database");
        seed_active(&db, store.as_ref(), now());
        let service = build_service(config, store.clone(), now());
        let error = service.deactivate(&db).await.unwrap_err();
        assert_eq!(error.code, "polar_unavailable");
        assert!(db.get_license_snapshot().expect("retained").is_some());
        assert!(store.get("credential").is_ok());
        thread.join().expect("server");

        let (config, thread) = spawn_server(vec![(204, String::new())]);
        let service = build_service(config, store.clone(), now());
        assert_eq!(
            service.deactivate(&db).await.expect("deactivate").state,
            LicenseStatus::Unlicensed
        );
        assert!(db.get_license_snapshot().expect("cleared").is_none());
        assert_eq!(
            store.get("credential").unwrap_err(),
            LicenseStoreError::Missing
        );
        thread.join().expect("server");
    }

    #[test]
    fn unconfigured_status_is_local_safe_and_does_not_gate_app_data() {
        let service = LicenseService::unconfigured(None);
        let db = Db::open_in_memory().expect("database");
        let status = service.get_status(&db).expect("status");
        assert_eq!(status.state, LicenseStatus::NotConfigured);
        assert_eq!(status.product, LicenseProduct::None);
        assert!(status.masked_key.is_none());
        assert!(!service.should_refresh(&db));
    }

    #[test]
    fn every_frontend_state_serializes_without_secret_fields() {
        for state in [
            LicenseStatus::Unlicensed,
            LicenseStatus::Active,
            LicenseStatus::OfflineGrace,
            LicenseStatus::NeedsOnline,
            LicenseStatus::Expired,
            LicenseStatus::Revoked,
            LicenseStatus::Disabled,
            LicenseStatus::DeviceLimit,
            LicenseStatus::SecureStorageUnavailable,
            LicenseStatus::NotConfigured,
        ] {
            let dto = LicenseStatusDto {
                state,
                masked_key: Some("••••-CRET".into()),
                ..LicenseStatusDto::unlicensed()
            };
            let json = serde_json::to_string(&dto).expect("serialize DTO");
            assert!(!json.contains(KEY));
            assert!(!json.contains(ACTIVATION));
            assert!(!json.contains("credentialRef"));
            assert!(!json.contains("licenseKey"));
            assert!(!json.contains("activationId"));
        }
        let error = LicenseCommandError::new("invalid_license", true);
        assert_eq!(
            format!("{error:?}"),
            "LicenseCommandError { code: \"invalid_license\", recoverable: true }"
        );
    }
}
