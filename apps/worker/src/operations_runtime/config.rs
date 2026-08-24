use nopager_policy::SafetyMode;

pub(super) fn configured_docker_target() -> Option<String> {
    std::env::var("NOPAGER_DOCKER_TARGET")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub(super) fn docker_restart_enabled() -> bool {
    std::env::var("NOPAGER_DOCKER_RESTART_ENABLED")
        .ok()
        .is_some_and(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

pub(super) fn parse_safety_mode(value: &str) -> anyhow::Result<SafetyMode> {
    match value {
        "safe" => Ok(SafetyMode::Safe),
        "autopilot_experimental" | "autopilot-experimental" => {
            Ok(SafetyMode::AutopilotExperimental)
        }
        _ => anyhow::bail!("unknown safety mode: {value}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mutation_opt_in_values_are_explicit() {
        assert!(matches!("true", "1" | "true" | "TRUE" | "yes" | "YES"));
        assert!(!matches!("false", "1" | "true" | "TRUE" | "yes" | "YES"));
    }
}
