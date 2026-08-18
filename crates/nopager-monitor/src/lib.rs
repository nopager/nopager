use std::{
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

use ipnet::IpNet;
use thiserror::Error;
use url::{Host, Url};

const VERCEL_BYPASS_ENV: &str = "VERCEL_AUTOMATION_BYPASS_SECRET";
const VERCEL_BYPASS_HEADER: &str = "x-vercel-protection-bypass";

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
    // Keep this denylist conservative for an SSRF boundary. It covers private,
    // loopback, link-local, shared, benchmarking, documentation, translation,
    // multicast, reserved, and other non-public/special-use ranges that should
    // never be valid production health-check destinations.
    const BLOCKED: &[&str] = &[
        "0.0.0.0/8",
        "10.0.0.0/8",
        "100.64.0.0/10",
        "127.0.0.0/8",
        "169.254.0.0/16",
        "172.16.0.0/12",
        "192.0.0.0/24",
        "192.0.2.0/24",
        "192.88.99.0/24",
        "192.168.0.0/16",
        "198.18.0.0/15",
        "198.51.100.0/24",
        "203.0.113.0/24",
        "224.0.0.0/4",
        "240.0.0.0/4",
        "::/128",
        "::1/128",
        "::ffff:0:0/96",
        "64:ff9b::/96",
        "64:ff9b:1::/48",
        "100::/64",
        "100:0:0:1::/64",
        "2001::/32",
        "2001:2::/48",
        "2001:10::/28",
        "2001:db8::/32",
        "2002::/16",
        "3fff::/20",
        "5f00::/16",
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
    let started = Instant::now();
    let mut builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
    if url
        .host()
        .is_some_and(|value| matches!(value, Host::Domain(_)))
    {
        let dns_timeout = timeout.min(Duration::from_secs(5));
        let resolved: Vec<SocketAddr> =
            match tokio::time::timeout(dns_timeout, tokio::net::lookup_host((host, port))).await {
                Ok(Ok(addresses)) => addresses.collect(),
                Ok(Err(_)) => return Ok(failed_observation(started, "dns")),
                Err(_) => return Ok(failed_observation(started, "dns_timeout")),
            };
        validate_resolved_addresses(resolved.iter().map(SocketAddr::ip))?;
        // Pin the connection to the addresses we just validated. This prevents
        // a second DNS resolution from turning the validation/request gap into
        // a DNS-rebinding SSRF bypass.
        builder = builder.resolve_to_addrs(host, &resolved);
    }
    let remaining = timeout.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Ok(failed_observation(started, "timeout"));
    }
    let client = builder
        .connect_timeout(remaining.min(Duration::from_secs(5)))
        .timeout(remaining)
        .build()?;
    let mut request = client.get(url.clone());
    // Vercel preview deployments are frequently protected. Vercel's official
    // automation bypass is intentionally injected only for *.vercel.app and
    // only when this process has been given the dedicated secret. In the
    // default Compose topology that environment variable is worker-only, so
    // setup/production health checks still prove the configured app is public.
    if is_vercel_deployment_host(host)
        && let Ok(secret) = std::env::var(VERCEL_BYPASS_ENV)
        && !secret.trim().is_empty()
    {
        request = request.header(VERCEL_BYPASS_HEADER, secret);
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            Ok(HttpHealthObservation {
                success: status == expected_status,
                status_code: Some(status),
                latency_ms: started.elapsed().as_millis(),
                error_class: (status != expected_status).then_some("unexpected_status"),
            })
        }
        Err(error) => Ok(failed_observation(
            started,
            if error.is_timeout() {
                "timeout"
            } else {
                "connection"
            },
        )),
    }
}

fn failed_observation(started: Instant, error_class: &'static str) -> HttpHealthObservation {
    HttpHealthObservation {
        success: false,
        status_code: None,
        latency_ms: started.elapsed().as_millis(),
        error_class: Some(error_class),
    }
}

fn is_vercel_deployment_host(host: &str) -> bool {
    host.trim_end_matches('.')
        .to_ascii_lowercase()
        .ends_with(".vercel.app")
}

#[derive(Debug, Error)]
pub enum HttpCheckError {
    #[error(transparent)]
    UnsafeUrl(#[from] UrlSafetyError),
    #[error("health check URL has no supported port")]
    UnsupportedPort,
    #[error(transparent)]
    Client(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_public_https_url() {
        assert!(validate_health_url(&Url::parse("https://example.com/health").unwrap()).is_ok());
        assert!(validate_public_ip("93.184.216.34".parse().unwrap()).is_ok());
        assert!(validate_public_ip("2606:2800:220:1:248:1893:25c8:1946".parse().unwrap()).is_ok());
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
    fn blocks_non_public_special_use_addresses() {
        for address in [
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.2",
            "203.0.113.3",
            "250.1.2.3",
            "::ffff:192.0.2.1",
            "64:ff9b::c000:0201",
            "2001:2::1",
            "2001:db8::1",
            "3fff::1",
            "5f00::1",
        ] {
            assert_eq!(
                validate_public_ip(address.parse().unwrap()),
                Err(UrlSafetyError::NonPublicTarget),
                "{address}"
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

    #[test]
    fn failed_observations_preserve_failure_class() {
        let observation = failed_observation(Instant::now(), "dns");
        assert!(!observation.success);
        assert_eq!(observation.status_code, None);
        assert_eq!(observation.error_class, Some("dns"));
    }

    #[test]
    fn sends_vercel_bypass_only_to_vercel_deployment_hosts() {
        assert!(is_vercel_deployment_host("demo-git-main-team.vercel.app"));
        assert!(is_vercel_deployment_host("DEMO.VERCEL.APP."));
        assert!(!is_vercel_deployment_host("vercel.app"));
        assert!(!is_vercel_deployment_host("vercel.app.attacker.example"));
        assert!(!is_vercel_deployment_host("example.com"));
    }
}
