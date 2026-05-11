//! Discovery scheduler. Decides when to run the next manifest check.
//!
//! Inputs are deliberately small: the current time, whether the app has
//! reached Settled Idle yet, the time of the last check, and the consecutive
//! failure count. The scheduler returns a [`NextAction`] which the caller
//! converts into "fetch now" or "wait".
//!
//! Cadence per ADR-0015:
//!
//! * First check after Settled Idle following startup (never blocks
//!   startup).
//! * Every 6 hours after a successful check.
//! * Exponential backoff after a failed check, capped at 6 hours.

use std::time::Duration;

/// 6 hours, the steady-state poll interval and also the backoff cap.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// First backoff after a single failure.
const INITIAL_BACKOFF: Duration = Duration::from_secs(60);

/// Multiplier for each consecutive failure.
const BACKOFF_FACTOR: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NextAction {
    /// Don't start a check yet — keep waiting for Settled Idle.
    WaitForSettledIdle,
    /// Run a check now.
    CheckNow,
    /// Wait at least this much before checking again.
    WaitFor(Duration),
}

/// Input snapshot passed to [`decide`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScheduleInput {
    /// Has the app reached its first Settled Idle since startup?
    pub settled_idle: bool,
    /// Monotonic instant since startup that we are scheduling against.
    pub now: Duration,
    /// `now` value at the start of the most recent check attempt, or
    /// `None` if no check has been attempted yet.
    pub last_check: Option<Duration>,
    /// Number of consecutive failures (0 if last check succeeded or no
    /// check has been attempted).
    pub consecutive_failures: u32,
}

pub(crate) fn decide(input: ScheduleInput) -> NextAction {
    if !input.settled_idle {
        return NextAction::WaitForSettledIdle;
    }
    match input.last_check {
        None => NextAction::CheckNow,
        Some(last) => {
            let elapsed = input.now.saturating_sub(last);
            let wait = if input.consecutive_failures == 0 {
                POLL_INTERVAL
            } else {
                backoff_for(input.consecutive_failures)
            };
            if elapsed >= wait {
                NextAction::CheckNow
            } else {
                NextAction::WaitFor(wait - elapsed)
            }
        }
    }
}

/// Exponential backoff capped at the steady-state poll interval.
fn backoff_for(consecutive_failures: u32) -> Duration {
    let shift = consecutive_failures.saturating_sub(1).min(16);
    let raw_secs = INITIAL_BACKOFF
        .as_secs()
        .saturating_mul(BACKOFF_FACTOR.saturating_pow(shift) as u64);
    Duration::from_secs(raw_secs).min(POLL_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(
        settled_idle: bool,
        now_secs: u64,
        last_check_secs: Option<u64>,
        failures: u32,
    ) -> ScheduleInput {
        ScheduleInput {
            settled_idle,
            now: Duration::from_secs(now_secs),
            last_check: last_check_secs.map(Duration::from_secs),
            consecutive_failures: failures,
        }
    }

    #[test]
    fn before_settled_idle_waits() {
        let action = decide(input(false, 1, None, 0));
        assert_eq!(action, NextAction::WaitForSettledIdle);
    }

    #[test]
    fn after_settled_idle_with_no_prior_check_runs_immediately() {
        let action = decide(input(true, 1, None, 0));
        assert_eq!(action, NextAction::CheckNow);
    }

    #[test]
    fn six_hours_after_success_runs_again() {
        let last = 100;
        let now = last + POLL_INTERVAL.as_secs();
        assert_eq!(
            decide(input(true, now, Some(last), 0)),
            NextAction::CheckNow
        );
    }

    #[test]
    fn before_six_hours_after_success_waits_remaining() {
        let last = 0;
        let now = POLL_INTERVAL.as_secs() - 60;
        assert_eq!(
            decide(input(true, now, Some(last), 0)),
            NextAction::WaitFor(Duration::from_secs(60))
        );
    }

    #[test]
    fn first_failure_backs_off_one_minute() {
        let last = 1000;
        // Just after the failure: 30 seconds elapsed, 30 seconds to wait.
        let action = decide(input(true, last + 30, Some(last), 1));
        assert_eq!(action, NextAction::WaitFor(Duration::from_secs(30)));
    }

    #[test]
    fn failure_backoff_grows_exponentially() {
        let last = 0u64;
        // After 1 failure: 60s
        assert_eq!(
            decide(input(true, last, Some(last), 1)),
            NextAction::WaitFor(Duration::from_secs(60))
        );
        // After 2 failures: 120s
        assert_eq!(
            decide(input(true, last, Some(last), 2)),
            NextAction::WaitFor(Duration::from_secs(120))
        );
        // After 3 failures: 240s
        assert_eq!(
            decide(input(true, last, Some(last), 3)),
            NextAction::WaitFor(Duration::from_secs(240))
        );
    }

    #[test]
    fn failure_backoff_caps_at_six_hours() {
        let last = 0u64;
        // A pile of failures must not push the wait above POLL_INTERVAL.
        let action = decide(input(true, last, Some(last), 100));
        assert_eq!(action, NextAction::WaitFor(POLL_INTERVAL));
    }

    #[test]
    fn backoff_elapsed_means_check_now() {
        let last = 0u64;
        // 60 seconds of backoff have fully elapsed.
        let action = decide(input(true, 60, Some(last), 1));
        assert_eq!(action, NextAction::CheckNow);
    }
}
