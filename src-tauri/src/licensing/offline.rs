use chrono::{DateTime, Utc};

use super::models::LicenseStatus;

pub const REFRESH_AFTER_DAYS: i64 = 7;
pub const OFFLINE_GRACE_DAYS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfflineEvaluation {
    pub state: LicenseStatus,
    pub should_refresh: bool,
}

/// Reads a cached snapshot without touching the network.
///
/// Both products are perpetual, so a passed update deadline never ends
/// entitlement: it moves a healthy license to `Expired`, which means "entitled,
/// update window closed". It never shortens the offline grace policy, and it
/// never overrides a state that is already reporting something else.
pub fn evaluate_cached_state(
    state: LicenseStatus,
    update_deadline: Option<DateTime<Utc>>,
    refresh_due_at: Option<DateTime<Utc>>,
    offline_deadline: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> OfflineEvaluation {
    if !state.is_previously_granted() {
        // A closed window is a settled fact, not a lapse: it neither consumes
        // grace nor decays into `NeedsOnline`. It keeps re-checking on the
        // ordinary cadence so a renewal is picked up on its own.
        let should_refresh = state == LicenseStatus::Expired
            && refresh_due_at.is_some_and(|due| now >= due);
        return OfflineEvaluation {
            state,
            should_refresh,
        };
    }

    if offline_deadline.is_some_and(|deadline| now > deadline) {
        return OfflineEvaluation {
            state: LicenseStatus::NeedsOnline,
            should_refresh: true,
        };
    }

    let should_refresh = refresh_due_at.is_some_and(|due| now >= due);
    if state == LicenseStatus::Active
        && update_deadline.is_some_and(|deadline| now >= deadline)
    {
        return OfflineEvaluation {
            state: LicenseStatus::Expired,
            should_refresh,
        };
    }

    OfflineEvaluation {
        state,
        should_refresh,
    }
}

/// An outage is never a verdict. It can only move a previously granted state
/// into offline grace, or out of it once the 30-day deadline passes; every
/// other state, `Expired` included, is returned untouched.
pub fn state_after_transient_failure(
    prior_state: LicenseStatus,
    offline_deadline: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> LicenseStatus {
    if !prior_state.is_previously_granted() {
        return prior_state;
    }
    if offline_deadline.is_some_and(|deadline| now <= deadline) {
        LicenseStatus::OfflineGrace
    } else {
        LicenseStatus::NeedsOnline
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn origin() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 1, 12, 0, 0).unwrap()
    }

    #[test]
    fn refresh_is_due_exactly_at_day_seven() {
        let start = origin();
        let due = start + Duration::days(REFRESH_AFTER_DAYS);
        let deadline = start + Duration::days(OFFLINE_GRACE_DAYS);
        for (moment, expected) in [
            (due - Duration::nanoseconds(1), false),
            (due, true),
            (due + Duration::nanoseconds(1), true),
        ] {
            assert_eq!(
                evaluate_cached_state(
                    LicenseStatus::Active,
                    None,
                    Some(due),
                    Some(deadline),
                    moment,
                )
                .should_refresh,
                expected
            );
        }
    }

    #[test]
    fn transient_failure_allows_grace_through_exact_day_thirty() {
        let start = origin();
        let deadline = start + Duration::days(OFFLINE_GRACE_DAYS);
        for (moment, expected) in [
            (deadline - Duration::nanoseconds(1), LicenseStatus::OfflineGrace),
            (deadline, LicenseStatus::OfflineGrace),
            (
                deadline + Duration::nanoseconds(1),
                LicenseStatus::NeedsOnline,
            ),
        ] {
            assert_eq!(
                state_after_transient_failure(LicenseStatus::Active, Some(deadline), moment),
                expected
            );
        }
    }

    #[test]
    fn an_outage_never_moves_a_restrictive_or_expired_state() {
        let now = origin();
        for state in [
            LicenseStatus::Unlicensed,
            LicenseStatus::NeedsOnline,
            LicenseStatus::Expired,
            LicenseStatus::Revoked,
            LicenseStatus::Disabled,
            LicenseStatus::DeviceLimit,
        ] {
            assert_eq!(
                state_after_transient_failure(state, Some(now + Duration::days(30)), now),
                state
            );
        }
    }

    #[test]
    fn a_passed_deadline_closes_the_window_without_ending_the_license() {
        let now = origin();
        let evaluation = evaluate_cached_state(
            LicenseStatus::Active,
            Some(now),
            Some(now),
            Some(now + Duration::days(30)),
            now,
        );

        assert_eq!(evaluation.state, LicenseStatus::Expired);
        assert!(evaluation.state.is_entitled());
        assert!(evaluation.should_refresh);

        // The deadline belongs to the window, not to the offline policy: an
        // outage still hands the same license its full grace.
        assert_eq!(
            state_after_transient_failure(
                LicenseStatus::Active,
                Some(now + Duration::days(30)),
                now,
            ),
            LicenseStatus::OfflineGrace
        );
    }

    #[test]
    fn a_closed_window_keeps_re_checking_so_a_renewal_is_picked_up() {
        let start = origin();
        let due = start + Duration::days(REFRESH_AFTER_DAYS);
        for (moment, expected) in [(start, false), (due, true), (start + Duration::days(400), true)]
        {
            let evaluation = evaluate_cached_state(
                LicenseStatus::Expired,
                Some(start),
                Some(due),
                Some(start + Duration::days(OFFLINE_GRACE_DAYS)),
                moment,
            );
            // Never decays into NeedsOnline, however long it sits there.
            assert_eq!(evaluation.state, LicenseStatus::Expired);
            assert_eq!(evaluation.should_refresh, expected);
        }
    }
}
