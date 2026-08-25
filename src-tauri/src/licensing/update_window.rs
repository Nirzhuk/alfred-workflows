//! The one rule that decides whether a build is inside a license's update
//! window. Kept here as a single pure function so no date arithmetic leaks
//! into the service or the UI.

use chrono::{DateTime, NaiveDate, Utc};

/// The release date of *this* build, baked in at compile time by `build.rs`
/// from the release workflow. Unset means a source build.
pub const BUILD_RELEASE_DATE: Option<&str> = option_env!("ALFRED_RELEASE_DATE");

/// A build is in-window when it was released on or before the license's update
/// deadline: `ALFRED_RELEASE_DATE <= licenseUpdateDeadline`.
///
/// Two inputs never close a window:
/// - an unset (or unparsable) release date, which is a source build;
/// - an absent deadline, which is a key that carries no window at all.
///
/// Being out of window is never a loss of entitlement. A customer's existing
/// install keeps every paid feature forever, because its release date does not
/// change; only a *newer* build can fall outside the window they bought.
pub fn is_in_update_window(release_date: Option<&str>, update_deadline: Option<&str>) -> bool {
    let (Some(released), Some(deadline)) = (
        parse_release_date(release_date),
        parse_deadline(update_deadline),
    ) else {
        return true;
    };
    // Compared as whole days: a build released on the deadline day is inside
    // the window whatever time of day the deadline itself carries.
    released <= deadline.date_naive()
}

/// ISO 8601 `YYYY-MM-DD`, the format the release workflow supplies. A blank or
/// malformed value is treated as unset rather than as a closed window: a typo
/// in a release variable must never lock a paying customer out.
fn parse_release_date(value: Option<&str>) -> Option<NaiveDate> {
    let value = value.map(str::trim).filter(|value| !value.is_empty())?;
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

fn parse_deadline(value: Option<&str>) -> Option<DateTime<Utc>> {
    let value = value.map(str::trim).filter(|value| !value.is_empty())?;
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEADLINE: &str = "2027-01-15T09:30:00Z";

    #[test]
    fn a_build_released_before_the_deadline_is_in_window() {
        assert!(is_in_update_window(Some("2026-08-20"), Some(DEADLINE)));
        assert!(is_in_update_window(Some("2027-01-14"), Some(DEADLINE)));
    }

    #[test]
    fn a_build_released_exactly_on_the_deadline_day_is_in_window() {
        // The rule is `<=`, and the deadline's time of day does not shorten
        // the last day of the window in either direction.
        assert!(is_in_update_window(Some("2027-01-15"), Some(DEADLINE)));
        assert!(is_in_update_window(
            Some("2027-01-15"),
            Some("2027-01-15T00:00:00Z")
        ));
        assert!(is_in_update_window(
            Some("2027-01-15"),
            Some("2027-01-15T23:59:59Z")
        ));
    }

    #[test]
    fn a_build_released_after_the_deadline_is_out_of_window() {
        assert!(!is_in_update_window(Some("2027-01-16"), Some(DEADLINE)));
        assert!(!is_in_update_window(Some("2030-06-01"), Some(DEADLINE)));
    }

    #[test]
    fn an_unset_release_date_is_a_source_build_and_never_locks() {
        assert!(is_in_update_window(None, Some(DEADLINE)));
        assert!(is_in_update_window(Some(""), Some(DEADLINE)));
        assert!(is_in_update_window(Some("   "), Some(DEADLINE)));
        // A malformed baked value fails open for the same reason.
        assert!(is_in_update_window(Some("15/01/2027"), Some(DEADLINE)));
        assert!(is_in_update_window(
            Some("2027-01-15T00:00:00Z"),
            Some(DEADLINE)
        ));
    }

    #[test]
    fn a_license_with_no_deadline_has_no_window_to_leave() {
        assert!(is_in_update_window(Some("2030-06-01"), None));
        assert!(is_in_update_window(Some("2030-06-01"), Some("")));
        assert!(is_in_update_window(Some("2030-06-01"), Some("not-a-date")));
        assert!(is_in_update_window(None, None));
    }

    #[test]
    fn a_deadline_in_a_local_offset_is_compared_in_utc() {
        // 2027-01-15T23:00:00+02:00 is 21:00Z on the 15th, so a build released
        // on the 15th is still in-window; the 16th is not.
        assert!(is_in_update_window(
            Some("2027-01-15"),
            Some("2027-01-15T23:00:00+02:00")
        ));
        assert!(!is_in_update_window(
            Some("2027-01-16"),
            Some("2027-01-15T23:00:00+02:00")
        ));
    }
}
