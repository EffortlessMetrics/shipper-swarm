use anyhow::{Result, bail};
use shipper_types::retry::{PerErrorConfig, RetryStrategyConfig};
use shipper_types::{ErrorClass, RuntimeOptions};

/// Effective retry policy for one classified failure.
///
/// The top-level runtime policy is the fallback when that class has no
/// override. CLI overrides have already been projected over every configured
/// class by `shipper-config` before the engine receives `RuntimeOptions`.
/// `override_configured` is retained because permanent failures remain
/// non-retryable unless the operator explicitly configured that class.
#[derive(Debug, Clone)]
pub(in crate::engine) struct RetryDecision {
    pub(in crate::engine) config: RetryStrategyConfig,
    pub(super) override_configured: bool,
}

impl RetryDecision {
    pub(super) fn permits_retry(&self, class: &ErrorClass, attempt: u32) -> bool {
        (class != &ErrorClass::Permanent || self.override_configured)
            && attempt < self.config.max_attempts
    }
}

pub(in crate::engine) fn validate_runtime_retry_options(opts: &RuntimeOptions) -> Result<()> {
    validate_config(
        "retry",
        &RetryStrategyConfig {
            strategy: opts.retry_strategy,
            max_attempts: opts.max_attempts,
            base_delay: opts.base_delay,
            max_delay: opts.max_delay,
            jitter: opts.retry_jitter,
        },
    )?;

    for (class_name, config) in [
        ("retryable", opts.retry_per_error.retryable.as_ref()),
        ("ambiguous", opts.retry_per_error.ambiguous.as_ref()),
        ("permanent", opts.retry_per_error.permanent.as_ref()),
    ] {
        if let Some(config) = config {
            validate_config(&format!("retry.per_error.{class_name}"), config)?;
        }
    }
    Ok(())
}

fn validate_config(path: &str, config: &RetryStrategyConfig) -> Result<()> {
    if config.max_attempts == 0 {
        bail!("{path}.max_attempts must be greater than 0");
    }
    if config.max_delay < config.base_delay {
        bail!("{path}.max_delay must be greater than or equal to base_delay");
    }
    if !(0.0..=1.0).contains(&config.jitter) {
        bail!("{path}.jitter must be between 0.0 and 1.0");
    }
    Ok(())
}

pub(in crate::engine) fn retry_decision(
    opts: &RuntimeOptions,
    class: &ErrorClass,
) -> RetryDecision {
    let fallback = RetryStrategyConfig {
        strategy: opts.retry_strategy,
        max_attempts: opts.max_attempts,
        base_delay: opts.base_delay,
        max_delay: opts.max_delay,
        jitter: opts.retry_jitter,
    };

    effective_retry_decision(fallback, &opts.retry_per_error, class)
}

fn effective_retry_decision(
    fallback: RetryStrategyConfig,
    per_error: &PerErrorConfig,
    class: &ErrorClass,
) -> RetryDecision {
    let override_config = match class {
        ErrorClass::Retryable => per_error.retryable.as_ref(),
        ErrorClass::Permanent => per_error.permanent.as_ref(),
        ErrorClass::Ambiguous => per_error.ambiguous.as_ref(),
    };
    let override_configured = override_config.is_some();
    let hard_ceiling = fallback.max_attempts.max(1);
    let mut config = override_config.cloned().unwrap_or(fallback);

    // Deserialized configuration rejects zero. Keep the engine boundary safe
    // for embedders that construct RuntimeOptions directly: one Cargo attempt
    // has already occurred before a classified retry decision exists. A class
    // override may narrow the cumulative package ceiling, but it must never
    // expand the top-level/CLI ceiling.
    config.max_attempts = config.max_attempts.max(1).min(hard_ceiling);

    RetryDecision {
        config,
        override_configured,
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Context, ensure};
    use std::time::Duration;

    use shipper_types::retry::RetryStrategyType;

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

    fn check_config_eq(actual: &RetryStrategyConfig, expected: &RetryStrategyConfig) -> Result<()> {
        ensure!(actual.strategy == expected.strategy);
        ensure!(actual.max_attempts == expected.max_attempts);
        ensure!(actual.base_delay == expected.base_delay);
        ensure!(actual.max_delay == expected.max_delay);
        ensure!((actual.jitter - expected.jitter).abs() < f64::EPSILON);
        Ok(())
    }

    #[test]
    fn unconfigured_class_uses_the_fallback_policy() -> Result<()> {
        let fallback = config(
            RetryStrategyType::Exponential,
            6,
            Duration::from_secs(2),
            Duration::from_mins(2),
            0.5,
        );

        let decision = effective_retry_decision(
            fallback.clone(),
            &PerErrorConfig::default(),
            &ErrorClass::Retryable,
        );

        check_config_eq(&decision.config, &fallback)?;
        ensure!(!decision.override_configured);
        ensure!(decision.permits_retry(&ErrorClass::Retryable, 5));
        ensure!(!decision.permits_retry(&ErrorClass::Retryable, 6));
        Ok(())
    }

    #[test]
    fn class_override_replaces_the_fallback_policy() -> Result<()> {
        let fallback = config(
            RetryStrategyType::Exponential,
            6,
            Duration::from_secs(3),
            Duration::from_secs(90),
            0.4,
        );
        let retryable = config(
            RetryStrategyType::Immediate,
            10,
            Duration::from_secs(1),
            Duration::from_secs(4),
            0.0,
        );
        let per_error = PerErrorConfig {
            retryable: Some(retryable.clone()),
            ambiguous: None,
            permanent: None,
        };

        let decision = effective_retry_decision(fallback, &per_error, &ErrorClass::Retryable);

        let mut expected = retryable;
        expected.max_attempts = 6;
        check_config_eq(&decision.config, &expected)?;
        ensure!(decision.override_configured);
        ensure!(decision.permits_retry(&ErrorClass::Retryable, 5));
        ensure!(!decision.permits_retry(&ErrorClass::Retryable, 6));
        Ok(())
    }

    #[test]
    fn class_override_may_narrow_the_hard_package_ceiling() -> Result<()> {
        let fallback = config(
            RetryStrategyType::Exponential,
            6,
            Duration::from_secs(2),
            Duration::from_mins(2),
            0.5,
        );
        let per_error = PerErrorConfig {
            retryable: None,
            ambiguous: Some(config(
                RetryStrategyType::Constant,
                2,
                Duration::from_secs(5),
                Duration::from_secs(5),
                0.0,
            )),
            permanent: None,
        };

        let decision = effective_retry_decision(fallback, &per_error, &ErrorClass::Ambiguous);

        ensure!(decision.config.max_attempts == 2);
        ensure!(decision.permits_retry(&ErrorClass::Ambiguous, 1));
        ensure!(!decision.permits_retry(&ErrorClass::Ambiguous, 2));
        Ok(())
    }

    #[test]
    fn direct_zero_attempt_contract_is_normalized_to_one() -> Result<()> {
        let fallback = config(
            RetryStrategyType::Linear,
            4,
            Duration::from_secs(2),
            Duration::from_secs(20),
            0.1,
        );
        let per_error = PerErrorConfig {
            retryable: None,
            ambiguous: Some(config(
                RetryStrategyType::Constant,
                0,
                Duration::from_secs(7),
                Duration::from_secs(7),
                0.0,
            )),
            permanent: None,
        };

        let decision = effective_retry_decision(fallback, &per_error, &ErrorClass::Ambiguous);

        ensure!(decision.config.max_attempts == 1);
        ensure!(!decision.permits_retry(&ErrorClass::Ambiguous, 1));
        Ok(())
    }

    #[test]
    fn runtime_policy_validation_rejects_invalid_values() -> Result<()> {
        let zero_attempts = config(
            RetryStrategyType::Immediate,
            0,
            Duration::ZERO,
            Duration::ZERO,
            0.0,
        );
        ensure!(validate_config("retry", &zero_attempts).is_err());

        let nan_jitter = config(
            RetryStrategyType::Immediate,
            2,
            Duration::ZERO,
            Duration::ZERO,
            f64::NAN,
        );
        ensure!(validate_config("retry", &nan_jitter).is_err());

        let invalid_bounds = config(
            RetryStrategyType::Immediate,
            2,
            Duration::from_secs(5),
            Duration::from_secs(4),
            0.0,
        );
        let error = validate_config("retry.per_error.ambiguous", &invalid_bounds)
            .err()
            .context("invalid class bounds were accepted")?;
        ensure!(
            error
                .to_string()
                .contains("retry.per_error.ambiguous.max_delay")
        );
        Ok(())
    }

    #[test]
    fn runtime_policy_validation_allows_zero_delay_immediate_class() -> Result<()> {
        let immediate = config(
            RetryStrategyType::Immediate,
            1,
            Duration::ZERO,
            Duration::ZERO,
            0.0,
        );
        validate_config("retry.per_error.permanent", &immediate)
    }

    #[test]
    fn permanent_failures_require_an_explicit_class_override() -> Result<()> {
        let fallback = config(
            RetryStrategyType::Exponential,
            6,
            Duration::from_secs(2),
            Duration::from_mins(2),
            0.5,
        );
        let none = effective_retry_decision(
            fallback.clone(),
            &PerErrorConfig::default(),
            &ErrorClass::Permanent,
        );
        ensure!(!none.permits_retry(&ErrorClass::Permanent, 1));

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
        let explicit = effective_retry_decision(fallback, &per_error, &ErrorClass::Permanent);
        ensure!(explicit.permits_retry(&ErrorClass::Permanent, 1));
        ensure!(!explicit.permits_retry(&ErrorClass::Permanent, 2));
        Ok(())
    }
}
