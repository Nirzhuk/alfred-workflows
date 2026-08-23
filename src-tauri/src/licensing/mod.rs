#[cfg(test)]
mod acceptance;
mod client;
mod config;
mod models;
mod offline;
mod service;
mod store;
mod update_window;

pub use models::{LicenseCommandError, LicenseStatusDto};
use service::LicenseService;

use crate::db::Db;
use std::future::Future;

pub struct LicensingState {
    service: LicenseService,
    single_flight: tokio::sync::Mutex<()>,
}

impl Default for LicensingState {
    fn default() -> Self {
        Self {
            service: LicenseService::load(),
            single_flight: tokio::sync::Mutex::new(()),
        }
    }
}

impl LicensingState {
    pub fn get_status(&self, db: &Db) -> Result<LicenseStatusDto, LicenseCommandError> {
        self.service.get_status(db)
    }

    pub fn should_refresh(&self, db: &Db) -> bool {
        self.service.should_refresh(db)
    }

    pub async fn activate(
        &self,
        db: &Db,
        license_key: String,
        device_label: String,
    ) -> Result<LicenseStatusDto, LicenseCommandError> {
        let mut license_key = zeroize::Zeroizing::new(license_key);
        self.run_single_flight(async {
            self.service
                .activate(db, std::mem::take(&mut *license_key), device_label)
                .await
        })
        .await
    }

    pub async fn refresh(&self, db: &Db) -> Result<LicenseStatusDto, LicenseCommandError> {
        self.run_single_flight(self.service.refresh(db)).await
    }

    pub async fn deactivate(&self, db: &Db) -> Result<LicenseStatusDto, LicenseCommandError> {
        self.run_single_flight(self.service.deactivate(db)).await
    }

    async fn run_single_flight<T>(&self, operation: impl Future<Output = T>) -> T {
        let _guard = self.single_flight.lock().await;
        operation.await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn shared_gate_keeps_license_operations_single_flight() {
        let state = LicensingState::default();
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));

        let operation = || {
            let active = active.clone();
            let maximum = maximum.clone();
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(30)).await;
                active.fetch_sub(1, Ordering::SeqCst);
            }
        };

        tokio::join!(
            state.run_single_flight(operation()),
            state.run_single_flight(operation())
        );

        assert_eq!(maximum.load(Ordering::SeqCst), 1);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}
