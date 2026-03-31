//! Observability wiring: structured metrics (task #48) and OTEL stubs (task #47).
//!
//! # Metrics (always available)
//!
//! The `metrics` crate facade is used unconditionally. Any recorder installed
//! by the application (e.g. the Prometheus exporter below) will receive these
//! measurements automatically.
//!
//! Metric naming follows the `rswarm_*` prefix convention to avoid collisions.
//!
//! # Prometheus exporter (feature `metrics-export`)
//!
//! Enable with `--features metrics-export`. Call `install_prometheus_recorder()`
//! once at process start. The recorder is global; repeated calls are a no-op.
//!
//! # OpenTelemetry (feature `otel`)
//!
//! Enable with `--features otel`. Call `init_tracer()` once at process start; it reads
//! `OTEL_EXPORTER_OTLP_ENDPOINT` from the environment.

// ---------------------------------------------------------------------------
// Metric name constants
// ---------------------------------------------------------------------------

/// Total number of agent loop iterations completed.
pub const METRIC_ITERATION_TOTAL: &str = "rswarm_iteration_total";
/// Cumulative tokens used (prompt + completion).
pub const METRIC_TOKEN_USAGE: &str = "rswarm_token_usage_total";
/// Tool call latency in milliseconds (histogram).
pub const METRIC_TOOL_LATENCY_MS: &str = "rswarm_tool_latency_ms";
/// Tool call outcome counter (label: tool_name, outcome = ok|err).
pub const METRIC_TOOL_OUTCOME: &str = "rswarm_tool_outcome_total";
/// Circuit breaker state change counter (label: breaker_name, state).
pub const METRIC_CIRCUIT_BREAKER_STATE: &str = "rswarm_circuit_breaker_state_total";
/// Guardrail trigger counter (label: guardrail_type).
pub const METRIC_GUARDRAIL_TRIGGERED: &str = "rswarm_guardrail_triggered_total";
/// LLM request latency in milliseconds (histogram).
pub const METRIC_LLM_LATENCY_MS: &str = "rswarm_llm_latency_ms";
/// Budget exhaustion counter (label: limit_type).
pub const METRIC_BUDGET_EXHAUSTED: &str = "rswarm_budget_exhausted_total";

// ---------------------------------------------------------------------------
// Metric recording helpers
// ---------------------------------------------------------------------------

/// Increment the iteration counter for the given agent.
pub fn record_iteration(agent_name: &str) {
    metrics::counter!(METRIC_ITERATION_TOTAL, "agent" => agent_name.to_string()).increment(1);
}

/// Add to the cumulative token usage counter.
pub fn record_token_usage(tokens: u64, model: &str) {
    metrics::counter!(METRIC_TOKEN_USAGE, "model" => model.to_string()).increment(tokens);
}

/// Record a single tool call's latency and outcome.
pub fn record_tool_call(tool_name: &str, latency_ms: f64, success: bool) {
    metrics::histogram!(METRIC_TOOL_LATENCY_MS, "tool" => tool_name.to_string()).record(latency_ms);
    let outcome = if success { "ok" } else { "err" };
    metrics::counter!(
        METRIC_TOOL_OUTCOME,
        "tool"    => tool_name.to_string(),
        "outcome" => outcome
    )
    .increment(1);
}

/// Record a circuit breaker state transition.
pub fn record_circuit_breaker_state(breaker_name: &str, state: &str) {
    metrics::counter!(
        METRIC_CIRCUIT_BREAKER_STATE,
        "breaker" => breaker_name.to_string(),
        "state"   => state.to_string()
    )
    .increment(1);
}

/// Increment the guardrail trigger counter for a named guardrail type.
pub fn record_guardrail_triggered(guardrail_type: &str) {
    metrics::counter!(
        METRIC_GUARDRAIL_TRIGGERED,
        "type" => guardrail_type.to_string()
    )
    .increment(1);
}

/// Record a single LLM round-trip latency.
pub fn record_llm_latency(latency_ms: f64, model: &str) {
    metrics::histogram!(METRIC_LLM_LATENCY_MS, "model" => model.to_string()).record(latency_ms);
}

/// Increment the budget-exhaustion counter for the given limit type.
pub fn record_budget_exhausted(limit_type: &str) {
    metrics::counter!(
        METRIC_BUDGET_EXHAUSTED,
        "limit_type" => limit_type.to_string()
    )
    .increment(1);
}

// ---------------------------------------------------------------------------
// Prometheus exporter (feature = "metrics-export")
// ---------------------------------------------------------------------------

/// Install a global Prometheus metrics recorder.
///
/// Must be called once, before any metrics are recorded. Subsequent calls are
/// safe but have no effect (the recorder is already installed).
///
/// Enables the `metrics-export` feature in Cargo.toml to activate.
#[cfg(feature = "metrics-export")]
pub fn install_prometheus_recorder(
) -> Result<metrics_exporter_prometheus::PrometheusHandle, metrics_exporter_prometheus::BuildError>
{
    use metrics_exporter_prometheus::PrometheusBuilder;

    PrometheusBuilder::new().install_recorder()
}

// ---------------------------------------------------------------------------
// OpenTelemetry trace/metrics export (feature = "otel")
// ---------------------------------------------------------------------------

/// Configuration for the OpenTelemetry exporter.
#[derive(Clone, Debug)]
pub struct OtelConfig {
    /// OTLP endpoint URL. Defaults to `OTEL_EXPORTER_OTLP_ENDPOINT` env var.
    pub endpoint: Option<String>,
    /// Service name sent in resource attributes.
    pub service_name: String,
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok(),
            service_name: "rswarm".to_string(),
        }
    }
}

/// Initialise the global OpenTelemetry tracer.
///
/// Installs an OTLP-backed OpenTelemetry layer for the global `tracing`
/// subscriber. Repeated calls after a successful initialization are no-ops.
#[cfg(feature = "otel")]
pub fn init_tracer(config: OtelConfig) -> crate::error::SwarmResult<()> {
    use std::sync::OnceLock;

    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_otlp::WithExportConfig;
    use tracing_opentelemetry::OpenTelemetryLayer;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    static INITIALIZED: OnceLock<()> = OnceLock::new();

    if INITIALIZED.get().is_some() {
        return Ok(());
    }

    let exporter_builder = opentelemetry_otlp::SpanExporter::builder().with_tonic();
    let exporter = if let Some(endpoint) = config
        .endpoint
        .as_deref()
        .filter(|endpoint| !endpoint.trim().is_empty())
    {
        exporter_builder.with_endpoint(endpoint.to_string()).build()
    } else {
        exporter_builder.build()
    }
    .map_err(|error| {
        crate::error::SwarmError::ConfigError(format!(
            "failed to build OTLP span exporter: {}",
            error
        ))
    })?;

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name(config.service_name.clone())
                .build(),
        )
        .with_batch_exporter(exporter)
        .build();
    let tracer = provider.tracer(config.service_name);

    tracing_subscriber::registry()
        .with(OpenTelemetryLayer::new(tracer))
        .try_init()
        .map_err(|error| {
            crate::error::SwarmError::ConfigError(format!(
                "failed to install tracing subscriber: {}",
                error
            ))
        })?;

    opentelemetry::global::set_tracer_provider(provider);
    let _ = INITIALIZED.set(());
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_constants_are_stable() {
        // Metric names are stable API surface — assert they haven't changed.
        assert_eq!(METRIC_ITERATION_TOTAL, "rswarm_iteration_total");
        assert_eq!(METRIC_TOKEN_USAGE, "rswarm_token_usage_total");
        assert_eq!(METRIC_TOOL_LATENCY_MS, "rswarm_tool_latency_ms");
        assert_eq!(
            METRIC_GUARDRAIL_TRIGGERED,
            "rswarm_guardrail_triggered_total"
        );
        assert_eq!(
            METRIC_CIRCUIT_BREAKER_STATE,
            "rswarm_circuit_breaker_state_total"
        );
        assert_eq!(METRIC_BUDGET_EXHAUSTED, "rswarm_budget_exhausted_total");
    }

    #[test]
    fn test_record_helpers_do_not_panic() {
        // Without a recorder installed these are no-ops, but must not panic.
        record_iteration("test-agent");
        record_token_usage(100, "gpt-4o");
        record_tool_call("my_tool", 42.5, true);
        record_circuit_breaker_state("provider-x", "open");
        record_guardrail_triggered("injection");
        record_llm_latency(150.0, "gpt-4o");
        record_budget_exhausted("token_budget");
    }

    #[test]
    fn test_otel_config_defaults() {
        let cfg = OtelConfig::default();
        assert_eq!(cfg.service_name, "rswarm");
    }
}
