use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::update_window::{is_in_update_window, BUILD_RELEASE_DATE};
use crate::db::StoredLicenseSnapshot;

/// The two products Alfred sells. Both are one-time purchases that unlock
/// every pro feature permanently; the license's update deadline bounds which
/// *builds* carry that entitlement, never the entitlement itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LicenseProduct {
    None,
    Individual,
    Teams,
}

/// Stored values written by the superseded four-product model, and the class
/// each one becomes. Kept explicit so no legacy row can drift into the wrong
/// product: `desktopAnnual` and `desktopLifetime` were both the single named
/// user, and `companySeat` was the seat-based product.
const LEGACY_DB_VALUES: [(&str, LicenseProduct); 3] = [
    ("desktopAnnual", LicenseProduct::Individual),
    ("desktopLifetime", LicenseProduct::Individual),
    ("companySeat", LicenseProduct::Teams),
];

/// A stored product value that belongs to neither the current nor the legacy
/// vocabulary. It is reported rather than defaulted: guessing would hand a
/// customer a product they never bought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownLicenseProduct;

impl LicenseProduct {
    pub(crate) fn as_db_value(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Individual => "individual",
            Self::Teams => "teams",
        }
    }

    /// Reads a stored value, migrating the legacy four-product names on the
    /// way. Anything else is an error, never a silent `None`.
    pub(crate) fn from_db_value(value: &str) -> Result<Self, UnknownLicenseProduct> {
        match value {
            "none" => Ok(Self::None),
            "individual" => Ok(Self::Individual),
            "teams" => Ok(Self::Teams),
            legacy => LEGACY_DB_VALUES
                .iter()
                .find(|(name, _)| *name == legacy)
                .map(|(_, product)| *product)
                .ok_or(UnknownLicenseProduct),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LicenseStatus {
    Unlicensed,
    Active,
    OfflineGrace,
    NeedsOnline,
    Expired,
    Revoked,
    Disabled,
    DeviceLimit,
    SecureStorageUnavailable,
    NotConfigured,
}

impl LicenseStatus {
    pub(crate) fn as_db_value(self) -> &'static str {
        match self {
            Self::Unlicensed => "unlicensed",
            Self::Active => "active",
            Self::OfflineGrace => "offlineGrace",
            Self::NeedsOnline => "needsOnline",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
            Self::Disabled => "disabled",
            Self::DeviceLimit => "deviceLimit",
            Self::SecureStorageUnavailable => "secureStorageUnavailable",
            Self::NotConfigured => "notConfigured",
        }
    }

    pub(crate) fn from_db_value(value: &str) -> Self {
        match value {
            "active" => Self::Active,
            "offlineGrace" => Self::OfflineGrace,
            "needsOnline" => Self::NeedsOnline,
            "expired" => Self::Expired,
            "revoked" => Self::Revoked,
            "disabled" => Self::Disabled,
            "deviceLimit" => Self::DeviceLimit,
            "secureStorageUnavailable" => Self::SecureStorageUnavailable,
            "notConfigured" => Self::NotConfigured,
            _ => Self::Unlicensed,
        }
    }

    pub(crate) fn is_previously_granted(self) -> bool {
        matches!(self, Self::Active | Self::OfflineGrace)
    }

    /// Whether a license key is recognized on this device, which is what the
    /// customer paid for. `Expired` means the update window closed, not that
    /// access ended, so it stays entitled; `NeedsOnline` is a pending
    /// revalidation, not a verdict. `Revoked` and `Disabled` are the two
    /// verdicts that do end entitlement, and must never be folded in with the
    /// other three.
    ///
    /// Plan 008 owns the gating that consumes this; here it is only asserted.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_entitled(self) -> bool {
        matches!(
            self,
            Self::Active | Self::OfflineGrace | Self::NeedsOnline | Self::Expired
        )
    }
}

/// The complete and intentionally redacted frontend contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseStatusDto {
    pub product: LicenseProduct,
    pub state: LicenseStatus,
    pub masked_key: Option<String>,
    pub benefit_id: Option<String>,
    pub activation_label: Option<String>,
    pub current_device: bool,
    /// The last date whose builds this license covers, straight from Polar's
    /// validation response. A date, never a key. `None` is a license with no
    /// window at all.
    pub update_deadline: Option<String>,
    /// Whether *this* build was released on or before `update_deadline`.
    /// Derived, never stored: it depends on the running build, not the row.
    pub in_update_window: bool,
    pub last_successful_validation: Option<String>,
    pub next_refresh: Option<String>,
    pub offline_deadline: Option<String>,
    pub error_code: Option<String>,
}

impl LicenseStatusDto {
    pub fn unlicensed() -> Self {
        Self {
            product: LicenseProduct::None,
            state: LicenseStatus::Unlicensed,
            masked_key: None,
            benefit_id: None,
            activation_label: None,
            current_device: false,
            update_deadline: None,
            in_update_window: true,
            last_successful_validation: None,
            next_refresh: None,
            offline_deadline: None,
            error_code: None,
        }
    }

    /// The only way the deadline is set, so the derived window answer can
    /// never fall out of step with it.
    pub(crate) fn set_update_deadline(&mut self, deadline: Option<String>) {
        self.in_update_window = is_in_update_window(BUILD_RELEASE_DATE, deadline.as_deref());
        self.update_deadline = deadline;
    }

    pub fn not_configured(error_code: Option<&str>) -> Self {
        Self {
            state: LicenseStatus::NotConfigured,
            error_code: error_code.map(str::to_owned),
            ..Self::unlicensed()
        }
    }

    /// Fails rather than defaults when the stored product is unreadable; see
    /// [`LicenseProduct::from_db_value`].
    pub(crate) fn from_stored(
        snapshot: &StoredLicenseSnapshot,
    ) -> Result<Self, UnknownLicenseProduct> {
        let mut dto = Self {
            product: LicenseProduct::from_db_value(&snapshot.product)?,
            state: LicenseStatus::from_db_value(&snapshot.status),
            masked_key: snapshot.masked_key.clone(),
            benefit_id: snapshot.benefit_id.clone(),
            activation_label: snapshot.activation_label.clone(),
            current_device: snapshot.current_device,
            last_successful_validation: snapshot.last_success_at.clone(),
            next_refresh: snapshot.refresh_due_at.clone(),
            offline_deadline: snapshot.offline_deadline.clone(),
            error_code: snapshot.error_code.clone(),
            ..Self::unlicensed()
        };
        dto.set_update_deadline(snapshot.expires_at.clone());
        Ok(dto)
    }

    pub(crate) fn into_stored(
        self,
        credential_ref: Option<String>,
        now: DateTime<Utc>,
    ) -> StoredLicenseSnapshot {
        StoredLicenseSnapshot {
            product: self.product.as_db_value().to_owned(),
            status: self.state.as_db_value().to_owned(),
            masked_key: self.masked_key,
            benefit_id: self.benefit_id,
            activation_label: self.activation_label,
            current_device: self.current_device,
            // The column keeps Polar's own name; the contract calls it what it
            // now means.
            expires_at: self.update_deadline,
            last_success_at: self.last_successful_validation,
            refresh_due_at: self.next_refresh,
            offline_deadline: self.offline_deadline,
            error_code: self.error_code,
            credential_ref,
            updated_at: now.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseCommandError {
    pub code: String,
    pub recoverable: bool,
}

impl LicenseCommandError {
    pub(crate) fn new(code: impl Into<String>, recoverable: bool) -> Self {
        Self {
            code: code.into(),
            recoverable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_legacy_product_value_migrates_to_exactly_one_current_class() {
        for (stored, expected) in [
            ("desktopAnnual", LicenseProduct::Individual),
            ("desktopLifetime", LicenseProduct::Individual),
            ("companySeat", LicenseProduct::Teams),
        ] {
            assert_eq!(
                LicenseProduct::from_db_value(stored),
                Ok(expected),
                "legacy `{stored}`"
            );
        }
    }

    #[test]
    fn current_product_values_round_trip_through_the_database() {
        for product in [
            LicenseProduct::None,
            LicenseProduct::Individual,
            LicenseProduct::Teams,
        ] {
            assert_eq!(
                LicenseProduct::from_db_value(product.as_db_value()),
                Ok(product)
            );
        }
        assert_eq!(LicenseProduct::None.as_db_value(), "none");
        assert_eq!(LicenseProduct::Individual.as_db_value(), "individual");
        assert_eq!(LicenseProduct::Teams.as_db_value(), "teams");
    }

    #[test]
    fn an_unknown_stored_product_is_rejected_rather_than_defaulted() {
        for stored in [
            "",
            "Individual",
            "desktop_annual",
            "companyseat",
            "enterprise",
            "null",
        ] {
            assert_eq!(
                LicenseProduct::from_db_value(stored),
                Err(UnknownLicenseProduct),
                "`{stored}` must not resolve to a product"
            );
        }
    }

    #[test]
    fn the_wire_contract_names_the_two_products_in_camel_case() {
        for (product, wire) in [
            (LicenseProduct::None, "\"none\""),
            (LicenseProduct::Individual, "\"individual\""),
            (LicenseProduct::Teams, "\"teams\""),
        ] {
            assert_eq!(serde_json::to_string(&product).expect("encode"), wire);
        }
        // The retired classes are not part of the contract in either direction.
        for retired in [
            "\"desktopAnnual\"",
            "\"desktopLifetime\"",
            "\"companySeat\"",
        ] {
            assert!(serde_json::from_str::<LicenseProduct>(retired).is_err());
        }
    }

    #[test]
    fn a_closed_update_window_keeps_entitlement_and_a_verdict_does_not() {
        for entitled in [
            LicenseStatus::Active,
            LicenseStatus::OfflineGrace,
            LicenseStatus::NeedsOnline,
            LicenseStatus::Expired,
        ] {
            assert!(entitled.is_entitled(), "{entitled:?}");
        }
        for ended in [
            LicenseStatus::Unlicensed,
            LicenseStatus::Revoked,
            LicenseStatus::Disabled,
            LicenseStatus::DeviceLimit,
            LicenseStatus::SecureStorageUnavailable,
            LicenseStatus::NotConfigured,
        ] {
            assert!(!ended.is_entitled(), "{ended:?}");
        }
    }

    #[test]
    fn a_legacy_snapshot_reads_as_the_migrated_product_and_an_unknown_one_errors() {
        let snapshot = StoredLicenseSnapshot {
            product: "companySeat".into(),
            status: "active".into(),
            masked_key: None,
            benefit_id: None,
            activation_label: None,
            current_device: true,
            expires_at: Some("2027-01-15T00:00:00Z".into()),
            last_success_at: None,
            refresh_due_at: None,
            offline_deadline: None,
            error_code: None,
            credential_ref: Some("credential".into()),
            updated_at: "2026-08-01T00:00:00Z".into(),
        };
        let dto = LicenseStatusDto::from_stored(&snapshot).expect("legacy snapshot");
        assert_eq!(dto.product, LicenseProduct::Teams);
        assert_eq!(dto.update_deadline.as_deref(), Some("2027-01-15T00:00:00Z"));
        // A source build (no baked release date) is always inside the window.
        assert_eq!(
            dto.in_update_window,
            is_in_update_window(BUILD_RELEASE_DATE, Some("2027-01-15T00:00:00Z"))
        );

        // Writing it back stores the migrated value, not the legacy one.
        let stored = dto
            .into_stored(Some("credential".into()), Utc::now())
            .product;
        assert_eq!(stored, "teams");

        let unknown = StoredLicenseSnapshot {
            product: "enterpriseSeat".into(),
            ..snapshot
        };
        assert_eq!(
            LicenseStatusDto::from_stored(&unknown),
            Err(UnknownLicenseProduct)
        );
    }
}
