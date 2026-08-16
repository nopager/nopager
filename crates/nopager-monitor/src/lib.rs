use std::{
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

use ipnet::IpNet;
use thiserror::Error;
use url::{Host, Url};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Unknown,
    Healthy,
    Failing,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthTransition {
    None,
    BecameHealthy,
    IncidentRequired,
    Recovered,
}

#[derive(Debug, Clone)]
pub struct HealthDebouncer {
    failure_threshold: u16,
    recovery_threshold: u16,
    consecutive_failures: u16,
    consecutive_successes: u16,
    status: HealthStatus,
}

impl Default for HealthDebouncer {
    fn default() -> Self {
        Self::new(3, 2)
    }
}

impl HealthDebouncer {
    #[must_use]
    pub const fn new(failure_threshold: u16, recovery_threshold: u16) -> Self {
        Self {
            failure_threshold,
            recovery_threshold,
            consecutive_failures: 0,
            consecutive_successes: 0,
            status: HealthStatus::Unknown,
        }
    }

    #[must_use]
    pub const fn status(&self) -> HealthStatus {
        self.status
    }

    pub fn observe(&mut self, result: CheckResult) -> HealthTransition {
        match result {
            CheckResult::Failure => {
                self.consecutive_successes = 0;
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                if self.status != HealthStatus::Down
                    && self.consecutive_failures >= self.failure_threshold
                {
                    self.status = HealthStatus::Down;
                    HealthTransition::IncidentRequired
                } else {
                    if self.status != HealthStatus::Down {
                        self.status = HealthStatus::Failing;
                    }
                    HealthTransition::None
                }
            }
            CheckResult::Success => {
                self.consecutive_failures = 0;
                self.consecutive_successes = self.consecutive_successes.saturating_add(1);
                if self.status == HealthStatus::Down
                    && self.consecutive_successes >= self.recovery_threshold
                {
                    self.status = HealthStatus::Healthy;
                    HealthTransition::Recovered
                } else if matches!(self.status, HealthStatus::Unknown | HealthStatus::Failing) {
                    self.status = HealthStatus::Healthy;
                    HealthTransition::BecameHealthy
                } else {
                    HealthTransition::None
                }
            }
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UrlSafetyError {
    #[error("health check URL must use HTTPS")]
    HttpsRequired,
    #[error("health check URL must include a public host")]
    MissingHost,
    #[error("health check target is not public")]
    NonPublicTarget,
    #[error("credentials in health check URLs are forbidden")]
    EmbeddedCredentials,
}

pub fn validate_health_url(url: &Url) -> Result<(), UrlSafetyError> {
    if url.scheme() != "https" {
        return Err(UrlSafetyError::HttpsRequired);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UrlSafetyError::EmbeddedCredentials);
    }
    match url.host().ok_or(UrlSafetyError::MissingHost)? {
        Host::Domain(host) => {
            if host.eq_ignore_ascii_case("localhost") || host.ends_with(".local") {
                return Err(UrlSafetyError::NonPublicTarget);
            }
        }
        Host::Ipv4(ip) => validate_public_ip(IpAddr::V4(ip))?,
        Host::Ipv6(ip) => validate_public_ip(IpAddr::V6(ip))?,
    }
    Ok(())
}

pub fn validate_public_ip(ip: IpAddr) -> Result<(), UrlSafetyError> {
    const BLOCKED: &[&str] = &[
        "0.0.0.0/8",
        "10.0.0.0/8",
        "100.64.0.0/10",
        "127.0.0.0/8",
        "169.254.0.0/16",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "224.0.0.0/4",
        "::/128",
        "::1/128",
        "fc00::/7",
        "fe80::/10",
        "ff00::/8",
    ];
    if BLOCKED
        .iter()
        .filter_map(|cidr| cidr.parse::<IpNet>().ok())
        .any(|network| network.contains(&ip))
    {
        return Err(UrlSafetyError::NonPublicTarget);
    }
    Ok(())
}

pub fn validate_resolved_addresses(
    addresses: impl IntoIterator<Item = IpAddr>,
) -> Result<(), UrlSafetyError> {
    let mut found = false;
    for address in addresses {
        found = true;
        validate_public_ip(address)?;
    }
    if !found {
        return Err(UrlSafetyError::MissingHost);
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct HttpHealthObservation {
    pub success: bool,
    pub status_code: Option<u16>,
    pub latency_ms: u128,
    pub error_class: Option<&'static str>,
}

pub async fn check_http(
    url: &Url,
    expected_status: u16,
    timeout: Duration,
) -> Result<HttpHealthObservation, HttpCheckError> {
    validate_health_url(url)?;
    let host = url.host_str().ok_or(UrlSafetyError::MissingHost)?;
    let port = url
        .port_or_known_default()
        .ok_or(HttpCheckError::UnsupportedPort)?;
    let mut builder = reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none());
    if url
        .host()
        .is_some_and(|value| matches!(value, Host::Domain(_)))
    {
        let resolved: Vec<SocketAddr> = tokio::net::lookup_host((host, port)).await?.collect();
        validate_resolved_addresses(resolved.iter().map(SocketAddr::ip))?;
        builder = builder.resolve_to_addrs(host, &resolved);
    }
    let client = builder.build()?;
    let started = Instant::now();
    match client.get(url.clone()).send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            Ok(HttpHealthObservation {
                success: status == expected_status,
                status_code: Some(status),
                latency_ms: started.elapsed().as_millis(),
                error_class: (status != expected_status).then_some("unexpected_status"),
            })
        }
        Err(error) => Ok(HttpHealthObservation {
            success: false,
            status_code: None,
            latency_ms: started.elapsed().as_millis(),
            error_class: Some(if error.is_timeout() {
                "timeout"
            } else {
                "connection"
            }),
        }),
    }
}

#[derive(Debug, Error)]
pub enum HttpCheckError {
    #[error(transparent)]
    UnsafeUrl(#[from] UrlSafetyError),
    #[error("health check URL has no supported port")]
    UnsupportedPort,
    #[error(transparent)]
    Dns(#[from] std::io::Error),
    #[error(transparent)]
    Client(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_public_https_url() {
        assert!(validate_health_url(&Url::parse("https://example.com/health").unwrap()).is_ok());
    }

    #[test]
    fn blocks_common_ssrf_targets() {
        for target in [
            "http://example.com",
            "https://localhost/health",
            "https://127.0.0.1/health",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/health",
        ] {
            assert!(
                validate_health_url(&Url::parse(target).unwrap()).is_err(),
                "{target}"
            );
        }
    }

    #[test]
    fn requires_three_failures_and_two_successes() {
        let mut health = HealthDebouncer::default();
        assert_eq!(health.observe(CheckResult::Failure), HealthTransition::None);
        assert_eq!(health.observe(CheckResult::Failure), HealthTransition::None);
        assert_eq!(
            health.observe(CheckResult::Failure),
            HealthTransition::IncidentRequired
        );
        assert_eq!(health.status(), HealthStatus::Down);
        assert_eq!(health.observe(CheckResult::Success), HealthTransition::None);
        assert_eq!(
            health.observe(CheckResult::Success),
            HealthTransition::Recovered
        );
        assert_eq!(health.status(), HealthStatus::Healthy);
    }

    #[test]
    fn blocks_if_any_dns_answer_is_private() {
        assert_eq!(
            validate_resolved_addresses([
                "93.184.216.34".parse().unwrap(),
                "127.0.0.1".parse().unwrap(),
            ]),
            Err(UrlSafetyError::NonPublicTarget)
        );
    }
}
