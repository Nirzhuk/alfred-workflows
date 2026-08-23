use rusqlite::{params, OptionalExtension};

use super::{Db, DbError};

/// Redacted, single-device licensing state. Secrets are addressed only by the
/// opaque credential reference and never enter SQLite.
///
/// `product` holds the `LicenseProduct` database vocabulary and `expires_at`
/// holds the license's update deadline; both are read back through
/// `LicenseStatusDto`, which rejects an unknown product rather than
/// defaulting it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLicenseSnapshot {
    pub product: String,
    pub status: String,
    pub masked_key: Option<String>,
    pub benefit_id: Option<String>,
    pub activation_label: Option<String>,
    pub current_device: bool,
    pub expires_at: Option<String>,
    pub last_success_at: Option<String>,
    pub refresh_due_at: Option<String>,
    pub offline_deadline: Option<String>,
    pub error_code: Option<String>,
    pub credential_ref: Option<String>,
    pub updated_at: String,
}

impl Db {
    pub fn get_license_snapshot(&self) -> Result<Option<StoredLicenseSnapshot>, DbError> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT product, status, masked_key, benefit_id, activation_label,
                        current_device, expires_at, last_success_at, refresh_due_at,
                        offline_deadline, error_code, credential_ref, updated_at
                   FROM license_snapshot WHERE id = 1",
                [],
                |row| {
                    Ok(StoredLicenseSnapshot {
                        product: row.get(0)?,
                        status: row.get(1)?,
                        masked_key: row.get(2)?,
                        benefit_id: row.get(3)?,
                        activation_label: row.get(4)?,
                        current_device: row.get::<_, i64>(5)? != 0,
                        expires_at: row.get(6)?,
                        last_success_at: row.get(7)?,
                        refresh_due_at: row.get(8)?,
                        offline_deadline: row.get(9)?,
                        error_code: row.get(10)?,
                        credential_ref: row.get(11)?,
                        updated_at: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(DbError::from)
        })
    }

    pub fn put_license_snapshot(&self, snapshot: &StoredLicenseSnapshot) -> Result<(), DbError> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO license_snapshot (
                    id, product, status, masked_key, benefit_id, activation_label,
                    current_device, expires_at, last_success_at, refresh_due_at,
                    offline_deadline, error_code, credential_ref, updated_at
                 ) VALUES (
                    1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
                 ) ON CONFLICT(id) DO UPDATE SET
                    product = excluded.product,
                    status = excluded.status,
                    masked_key = excluded.masked_key,
                    benefit_id = excluded.benefit_id,
                    activation_label = excluded.activation_label,
                    current_device = excluded.current_device,
                    expires_at = excluded.expires_at,
                    last_success_at = excluded.last_success_at,
                    refresh_due_at = excluded.refresh_due_at,
                    offline_deadline = excluded.offline_deadline,
                    error_code = excluded.error_code,
                    credential_ref = excluded.credential_ref,
                    updated_at = excluded.updated_at",
                params![
                    snapshot.product,
                    snapshot.status,
                    snapshot.masked_key,
                    snapshot.benefit_id,
                    snapshot.activation_label,
                    i64::from(snapshot.current_device),
                    snapshot.expires_at,
                    snapshot.last_success_at,
                    snapshot.refresh_due_at,
                    snapshot.offline_deadline,
                    snapshot.error_code,
                    snapshot.credential_ref,
                    snapshot.updated_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn delete_license_snapshot(&self) -> Result<(), DbError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM license_snapshot WHERE id = 1", [])?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> StoredLicenseSnapshot {
        StoredLicenseSnapshot {
            product: "individual".into(),
            status: "active".into(),
            masked_key: Some("••••-1234".into()),
            benefit_id: Some("11111111-1111-4111-8111-111111111111".into()),
            activation_label: Some("Test Mac".into()),
            current_device: true,
            expires_at: None,
            last_success_at: Some("2026-08-15T00:00:00Z".into()),
            refresh_due_at: Some("2026-08-22T00:00:00Z".into()),
            offline_deadline: Some("2026-09-14T00:00:00Z".into()),
            error_code: None,
            credential_ref: Some("license-opaque-reference".into()),
            updated_at: "2026-08-15T00:00:00Z".into(),
        }
    }

    #[test]
    fn snapshot_round_trip_contains_only_safe_fields() {
        let db = Db::open_in_memory().expect("open database");
        let snapshot = fixture();

        db.put_license_snapshot(&snapshot).expect("write snapshot");
        assert_eq!(
            db.get_license_snapshot().expect("read snapshot"),
            Some(snapshot)
        );

        db.with_conn(|conn| {
            let sql: String = conn.query_row(
                "SELECT group_concat(name, ',') FROM pragma_table_info('license_snapshot')",
                [],
                |row| row.get(0),
            )?;
            assert!(!sql.contains("license_key"));
            assert!(!sql.contains("activation_id"));
            Ok(())
        })
        .expect("inspect schema");
    }

    /// The row is opaque storage: it must accept a legacy value so the reader
    /// can migrate it, and must not rewrite or reject anything itself.
    #[test]
    fn a_legacy_product_value_survives_a_round_trip_for_the_reader_to_migrate() {
        let db = Db::open_in_memory().expect("open database");
        for stored in ["desktopAnnual", "desktopLifetime", "companySeat"] {
            let snapshot = StoredLicenseSnapshot {
                product: stored.into(),
                ..fixture()
            };
            db.put_license_snapshot(&snapshot).expect("write snapshot");
            assert_eq!(
                db.get_license_snapshot()
                    .expect("read snapshot")
                    .expect("row")
                    .product,
                stored
            );
        }
    }

    #[test]
    fn snapshot_is_single_row_and_can_be_cleared() {
        let db = Db::open_in_memory().expect("open database");
        let mut snapshot = fixture();
        db.put_license_snapshot(&snapshot).expect("first write");
        snapshot.status = "offlineGrace".into();
        db.put_license_snapshot(&snapshot).expect("second write");

        let count: i64 = db
            .with_conn(|conn| {
                conn.query_row("SELECT COUNT(*) FROM license_snapshot", [], |row| {
                    row.get(0)
                })
                .map_err(DbError::from)
            })
            .expect("count rows");
        assert_eq!(count, 1);
        assert_eq!(
            db.get_license_snapshot().expect("read").unwrap().status,
            "offlineGrace"
        );

        db.delete_license_snapshot().expect("delete");
        assert!(db.get_license_snapshot().expect("read empty").is_none());
    }
}
