use std::time::Duration;

use shipper_types::retry::{PerErrorConfig, RetryStrategyConfig, RetryStrategyType};
use shipper_types::{ErrorClass, RuntimeOptions};

/// Effective retry policy for one classified failure.
///
/// `config.max_attempts` is always bounded by the global cumulative ceiling.
/// `override_configured` is retained because permanent failures remain
/// non-retryable unless the operator explicitly configured that class.
#[derive(Debug, Clone)]
pub(super) struct RetryDecision {
    pub(super) config: RetryStrategyConfig,
    pub(super) override_configured: bool,
}

impl RetryDecision {
    pub(super) fn permits_retry(&self, class: &ErrorClass, attempt: u32) -> bool {
        (class != &ErrorClass::Permanent || self.override_configured)
            && attempt < self.config.max_attempts
    }
}

pub(super) fn retry_decision(opts: &RuntimeOptions, class: &ErrorClass) -> RetryDecision {
    let global = RetryStrategyConfig {
        strategy: opts.retry_strategy,
        max_attempts: opts.max_attempts,
        base_delay: opts.base_delay,
        max_delay: opts.max_delay,
        jitter: opts.retry_jitter,
    };

    effective_retry_decision(global, &opts.retry_per_error, class)
}

fn effective_retry_decision(
    global: RetryStrategyConfig,
    per_error: &PerErrorConfig,
    class: &ErrorClass,
) -> RetryDecision {
    let override_config = match class {
        ErrorClass::Retryable => per_error.retryable.as_ref(),
        ErrorClass::Permanent => per_error.permanent.as_ref(),
        ErrorClass::Ambiguous => per_error.ambiguous.as_ref(),
    };
    let override_configured = override_config.is_some();
    let global_max_attempts = global.max_attempts;
    let mut config = override_config.cloned().unwrap_or(global);

    // `max_attempts` is one cumulative package ceiling across publish and
    // resume. A class override may narrow it but cannot expand past the
    // top-level/CLI authority.
    config.max_attempts = config.max_attempts.min(global_max_attempts);

    RetryDecision {
        config,
        override_configured,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(
        strategy: RetryStrategyType,
        max_attempts: u32,
        base_delay: Duration,
        max_delay: Duration,
        jitter: f64,
    ) -> RetryStrategyConfig {
        RetryStrategyConfig {
            strategy,
            max_attempts,
            base_delay,
            max_delay,
            jitter,
        }
    }

    fn assert_config_eq(actual: &RetryStrategyConfig, expected: &RetryStrategyConfig) {
        assert_eq!(actual.strategy, expected.strategy);
        assert_eq!(actual.max_attempts, expected.max_attempts);
        assert_eq!(actual.base_delay, expected.base_delay);
        assert_eq!(actual.max_delay, expected.max_delay);
        assert!((actual.jitter - expected.jitter).abs() < f64::EPSILON);
    }

    #[test]
    fn unconfigured_class_uses_the_global_policy() {
        let global = config(
            RetryStrategyType::Exponential,
            6,
            Duration::from_secs(2),
            Duration::from_mins(2),
            0.5,
        );

        let decision = effective_retry_decision(
            global.clone(),
            &PerErrorConfig::default(),
            &ErrorClass::Retryable,
        );

        assert_config_eq(&decision.config, &global);
        assert!(!decision.override_configured);
        assert!(decision.permits_retry(&ErrorClass::Retryable, 5));
        assert!(!decision.permits_retry(&ErrorClass::Retryable, 6));
    }

    #[test]
    fn class_override_selects_its_strategy_and_narrows_the_ceiling() {
        let global = config(
            RetryStrategyType::Exponential,
            8,
            Duration::from_secs(3),
            Duration::from_secs(90),
            0.4,
        );
        let retryable = config(
            RetryStrategyType::Immediate,
            3,
            Duration::from_secs(1),
            Duration::from_secs(4),
            0.0,
        );
        let per_error = PerErrorConfig {
            retryable: Some(retryable.clone()),
            ambiguous: None,
            permanent: None,
        };

        let decision =
            effective_retry_decision(global, &per_error, &ErrorClass::Retryable);

        assert_config_eq(&decision.config, &retryable);
        assert!(decision.override_configured);
        assert!(decision.permits_retry(&ErrorClass::Retryable, 2));
        assert!(!decision.permits_retry(&ErrorClass::Retryable, 3));
    }

    #[test]
    fn class_override_cannot_expand_the_global_ceiling() {
        let global = config(
            RetryStrategyType::Linear,
            4,
            Duration::from_secs(2),
            Duration::from_secs(20),
            0.1,
        );
        let ambiguous = config(
            RetryStrategyType::Constant,
            12,
            Duration::from_secs(7),
            Duration::from_secs(7),
            0.0,
        );
        let per_error = PerErrorConfig {
            retryable: None,
            ambiguous: Some(ambiguous),
            permanent: None,
        };

        let decision =
            effective_retry_decision(global, &per_error, &ErrorClass::Ambiguous);

        assert_eq!(decision.config.max_attempts, 4);
        assert_eq!(decision.config.strategy, RetryStrategyType::Constant);
        assert_eq!(decision.config.base_delay, Duration::from_secs(7));
        assert!(decision.permits_retry(&ErrorClass::Ambiguous, 3));
        assert!(!decision.permits_retry(&ErrorClass::Ambiguous, 4));
    }

    #[test]
    fn permanent_failures_require_an_explicit_class_override() {
        let global = config(
            RetryStrategyType::Exponential,
            6,
            Duration::from_secs(2),
            Duration::from_mins(2),
            0.5,
        );
        let none = effective_retry_decision(
            global.clone(),
            &PerErrorConfig::default(),
            &ErrorClass::Permanent,
        );
        assert!(!none.permits_retry(&ErrorClass::Permanent, 1));

        let per_error = PerErrorConfig {
            retryable: None,
            ambiguous: None,
            permanent: Some(config(
                RetryStrategyType::Constant,
                2,
                Duration::from_secs(1),
                Duration::from_secs(1),
                0.0,
            )),
        };
        let explicit =
            effective_retry_decision(global, &per_error, &ErrorClass::Permanent);
        assert!(explicit.permits_retry(&ErrorClass::Permanent, 1));
        assert!(!explicit.permits_retry(&ErrorClass::Permanent, 2));
    }
}
