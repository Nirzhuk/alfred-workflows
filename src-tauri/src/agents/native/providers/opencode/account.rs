use crate::agents::native::{
    contains_cli_permission_flag, contains_secret_marker, NativeErrorCode, NativeRuntimeError,
};

pub const MAX_UPSTREAM_ID_BYTES: usize = 96;
pub const MAX_BILLING_OWNER_BYTES: usize = 128;
const MAX_MODEL_ID_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenCodeAuthKind {
    ProviderApiKey,
    ProviderOAuth,
    LocalProvider,
}

/// Non-secret identity that must accompany an opaque Alfred account reference.
///
/// `upstream_provider_id` is the OpenCode provider id used on the wire. It is
/// not inferred from a model catalog or from OpenCode's local auth store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeAccountBinding {
    upstream_provider_id: String,
    billing_owner: String,
    auth_kind: OpenCodeAuthKind,
}

impl OpenCodeAccountBinding {
    pub fn new(
        upstream_provider_id: impl Into<String>,
        billing_owner: impl Into<String>,
        auth_kind: OpenCodeAuthKind,
    ) -> Result<Self, NativeRuntimeError> {
        let upstream_provider_id = upstream_provider_id.into();
        let billing_owner = billing_owner.into();
        validate_component(
            &upstream_provider_id,
            MAX_UPSTREAM_ID_BYTES,
            "OpenCode upstream provider id",
        )?;
        validate_label(
            &billing_owner,
            MAX_BILLING_OWNER_BYTES,
            "OpenCode billing owner",
        )?;
        Ok(Self {
            upstream_provider_id,
            billing_owner,
            auth_kind,
        })
    }

    pub fn upstream_provider_id(&self) -> &str {
        &self.upstream_provider_id
    }

    pub fn billing_owner(&self) -> &str {
        &self.billing_owner
    }

    pub fn auth_kind(&self) -> OpenCodeAuthKind {
        self.auth_kind
    }

    pub fn validate_route(&self, route: &OpenCodeRoute) -> Result<(), NativeRuntimeError> {
        if route.upstream_provider_id != self.upstream_provider_id {
            return Err(NativeRuntimeError::new(
                NativeErrorCode::AccountMismatch,
                "OpenCode model route does not match the account's explicit upstream provider",
                false,
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCodeRoute {
    upstream_provider_id: String,
    model_id: String,
}

impl OpenCodeRoute {
    /// OpenCode's documented prompt contract uses separate `providerID` and
    /// `modelID` fields. Alfred stores them as the unambiguous
    /// `<provider-id>/<model-id>` string and splits only at the first slash.
    pub fn parse(value: &str) -> Result<Self, NativeRuntimeError> {
        let (upstream_provider_id, model_id) = value.split_once('/').ok_or_else(|| {
            NativeRuntimeError::new(
                NativeErrorCode::InvalidRequest,
                "OpenCode models must use the explicit upstream/model route format",
                false,
            )
        })?;
        validate_component(
            upstream_provider_id,
            MAX_UPSTREAM_ID_BYTES,
            "OpenCode upstream provider id",
        )?;
        validate_model_id(model_id)?;
        Ok(Self {
            upstream_provider_id: upstream_provider_id.into(),
            model_id: model_id.into(),
        })
    }

    pub fn upstream_provider_id(&self) -> &str {
        &self.upstream_provider_id
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn id(&self) -> String {
        format!("{}/{}", self.upstream_provider_id, self.model_id)
    }
}

fn validate_component(
    value: &str,
    max_bytes: usize,
    label: &str,
) -> Result<(), NativeRuntimeError> {
    let valid = !value.is_empty()
        && value.len() <= max_bytes
        && value.trim() == value
        && !contains_secret_marker(value)
        && !contains_cli_permission_flag(value)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'));
    if valid {
        Ok(())
    } else {
        Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            format!("{label} is invalid"),
            false,
        ))
    }
}

fn validate_label(value: &str, max_bytes: usize, label: &str) -> Result<(), NativeRuntimeError> {
    let valid = !value.is_empty()
        && value.len() <= max_bytes
        && value.trim() == value
        && !contains_secret_marker(value)
        && !contains_cli_permission_flag(value)
        && !value.chars().any(char::is_control);
    if valid {
        Ok(())
    } else {
        Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            format!("{label} is invalid"),
            false,
        ))
    }
}

fn validate_model_id(value: &str) -> Result<(), NativeRuntimeError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_MODEL_ID_BYTES
        && value.trim() == value
        && !contains_secret_marker(value)
        && !contains_cli_permission_flag(value)
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        });
    if valid {
        Ok(())
    } else {
        Err(NativeRuntimeError::new(
            NativeErrorCode::InvalidRequest,
            "OpenCode model id is invalid",
            false,
        ))
    }
}
