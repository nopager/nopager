//! Provider-neutral production signals used to wake NoPager's incident reasoning.
//!
//! These types intentionally do not depend on GitHub commits or deployment IDs.
//! Deployment events can be one signal source, but server/edge/database incidents
//! need a stable identity even when source code and deployment state are unchanged.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationalSignalKind {
    HttpHealth,
    TcpHealth,
    InstanceHealth,
    CpuPressure,
    MemoryPressure,
    DiskPressure,
    TrafficAnomaly,
    EdgeSecurity,
    DependencyHealth,
    DatabaseHealth,
    Deployment,
}

impl OperationalSignalKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HttpHealth => "http_health",
            Self::TcpHealth => "tcp_health",
            Self::InstanceHealth => "instance_health",
            Self::CpuPressure => "cpu_pressure",
            Self::MemoryPressure => "memory_pressure",
            Self::DiskPressure => "disk_pressure",
            Self::TrafficAnomaly => "traffic_anomaly",
            Self::EdgeSecurity => "edge_security",
            Self::DependencyHealth => "dependency_health",
            Self::DatabaseHealth => "database_health",
            Self::Deployment => "deployment",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProtectedResourceIdentity {
    /// Connector/provider family, for example `generic`, `cloudflare`, or a cloud provider.
    pub provider: String,
    /// Stable resource class, for example `service`, `vm`, `zone`, or `database`.
    pub resource_type: String,
    /// Stable provider/customer resource identity. It must not be an ephemeral deployment ID.
    pub resource_id: String,
}

impl ProtectedResourceIdentity {
    #[must_use]
    pub fn new(
        provider: impl Into<String>,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            resource_type: resource_type.into(),
            resource_id: resource_id.into(),
        }
    }

    #[must_use]
    pub fn incident_prefix(&self) -> String {
        format!(
            "ops:{}:{}:{}",
            normalize_key_part(&self.provider),
            normalize_key_part(&self.resource_type),
            normalize_key_part(&self.resource_id)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationalSignal {
    pub resource: ProtectedResourceIdentity,
    pub kind: OperationalSignalKind,
    /// Stable identity for the monitor/event source, not raw evidence.
    /// Examples: `primary_https_health`, `cloudflare_zone_traffic`, `vm_cpu`.
    pub source_id: String,
    /// Observation time recorded by the trusted monitoring layer.
    pub observed_at_unix_ms: u64,
    /// Whether this observation represents a failing/incident-producing condition.
    pub failing: bool,
}

impl OperationalSignal {
    #[must_use]
    pub fn new(
        resource: ProtectedResourceIdentity,
        kind: OperationalSignalKind,
        source_id: impl Into<String>,
        observed_at_unix_ms: u64,
        failing: bool,
    ) -> Self {
        Self {
            resource,
            kind,
            source_id: source_id.into(),
            observed_at_unix_ms,
            failing,
        }
    }

    /// Stable key for deduplicating one operational condition on one protected resource.
    ///
    /// The timestamp and provider-specific event/deployment IDs are intentionally excluded.
    /// Repeated observations of the same failing condition should converge on one incident.
    #[must_use]
    pub fn incident_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.resource.incident_prefix(),
            self.kind.as_str(),
            normalize_key_part(&self.source_id)
        )
    }
}

fn normalize_key_part(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':') {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_observations_share_one_incident_key() {
        let resource = ProtectedResourceIdentity::new("generic", "service", "api-primary");
        let first = OperationalSignal::new(
            resource.clone(),
            OperationalSignalKind::HttpHealth,
            "primary_https_health",
            1_000,
            true,
        );
        let second = OperationalSignal::new(
            resource,
            OperationalSignalKind::HttpHealth,
            "primary_https_health",
            2_000,
            true,
        );

        assert_eq!(first.incident_key(), second.incident_key());
    }

    #[test]
    fn resource_identity_is_independent_of_deployment_events() {
        let signal = OperationalSignal::new(
            ProtectedResourceIdentity::new("generic", "service", "checkout-api"),
            OperationalSignalKind::HttpHealth,
            "external_health",
            42,
            true,
        );

        assert_eq!(
            signal.incident_key(),
            "ops:generic:service:checkout-api:http_health:external_health"
        );
        assert!(!signal.incident_key().contains("vercel"));
        assert!(!signal.incident_key().contains("deployment"));
    }

    #[test]
    fn different_signal_classes_do_not_collapse_into_one_incident_key() {
        let resource = ProtectedResourceIdentity::new("cloud", "vm", "vm-123");
        let cpu = OperationalSignal::new(
            resource.clone(),
            OperationalSignalKind::CpuPressure,
            "provider_metrics",
            10,
            true,
        );
        let health = OperationalSignal::new(
            resource,
            OperationalSignalKind::HttpHealth,
            "primary_https_health",
            10,
            true,
        );

        assert_ne!(cpu.incident_key(), health.incident_key());
    }

    #[test]
    fn normalizes_user_configured_identity_for_stable_keys() {
        let resource =
            ProtectedResourceIdentity::new(" Generic Cloud ", "Service", "API / Primary");
        let signal = OperationalSignal::new(
            resource,
            OperationalSignalKind::HttpHealth,
            "Main Health",
            10,
            true,
        );

        assert_eq!(
            signal.incident_key(),
            "ops:generic_cloud:service:api___primary:http_health:main_health"
        );
    }
}
