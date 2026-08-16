use std::{fs, process::Command as ProcessCommand};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use clap::{Parser, Subcommand};
use nopager_config::ServerConfig;
use rand::RngCore;
use reqwest::StatusCode;
use serde_json::Value;

#[derive(Parser)]
#[command(name = "nopager", version, about = "NoPager self-hosting CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a safe local .env and continue setup in the web UI.
    Init,
    /// Validate local dependencies, configuration, and API reachability.
    Doctor,
    /// Show current protection and health state.
    Status,
    /// Open the single-app protection setup flow.
    Protect,
    /// List recent incidents.
    Incidents,
    /// Follow Docker Compose logs.
    Logs,
    /// Activate the kill switch while retaining monitoring.
    Pause,
    /// Resume mutations after a kill switch pause.
    Resume,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Init => init()?,
        Command::Doctor => doctor().await?,
        Command::Status => print_get("api/v1/overview").await?,
        Command::Protect => println!("Continue setup at {}/setup", web_url()),
        Command::Incidents => print_get("api/v1/incidents").await?,
        Command::Logs => logs()?,
        Command::Pause => mutate("api/v1/protection/pause").await?,
        Command::Resume => mutate("api/v1/protection/resume").await?,
    }
    Ok(())
}

fn init() -> anyhow::Result<()> {
    let mut master = [0_u8; 32];
    let mut admin = [0_u8; 32];
    rand::rng().fill_bytes(&mut master);
    rand::rng().fill_bytes(&mut admin);
    let existing = match fs::read_to_string(".env") {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            "DATABASE_URL=postgresql://nopager:nopager@localhost:5432/nopager\nNOPAGER_API_URL=http://localhost:8080/\n".into()
        }
        Err(error) => return Err(error.into()),
    };
    let (contents, master_changed) =
        fill_blank_env(existing, "NOPAGER_MASTER_KEY", &STANDARD.encode(master));
    let (contents, admin_changed) =
        fill_blank_env(contents, "NOPAGER_ADMIN_TOKEN", &STANDARD.encode(admin));
    if master_changed || admin_changed || !std::path::Path::new(".env").exists() {
        fs::write(".env", contents)?;
        println!("Created or completed .env with fresh local secrets.");
    } else {
        println!(".env already contains local secrets; left them unchanged.");
    }
    println!("Continue setup at {}/setup", web_url());
    Ok(())
}

fn fill_blank_env(mut contents: String, name: &str, value: &str) -> (String, bool) {
    let prefix = format!("{name}=");
    let mut found = false;
    let mut changed = false;
    let lines = contents
        .lines()
        .map(|line| {
            if let Some(current) = line.strip_prefix(&prefix) {
                found = true;
                if current.trim().is_empty() {
                    changed = true;
                    format!("{prefix}{value}")
                } else {
                    line.to_owned()
                }
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>();
    contents = lines.join("\n");
    if !found {
        if !contents.is_empty() {
            contents.push('\n');
        }
        contents.push_str(&prefix);
        contents.push_str(value);
        changed = true;
    }
    contents.push('\n');
    (contents, changed)
}

async fn doctor() -> anyhow::Result<()> {
    let mut healthy = true;
    match ServerConfig::from_env() {
        Ok(config) => println!("✓ server address: {}", config.address),
        Err(error) => {
            healthy = false;
            eprintln!("✗ server configuration: {error}");
        }
    }
    for program in ["node", "docker", "psql"] {
        if ProcessCommand::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            println!("✓ {program} available");
        } else {
            healthy = false;
            eprintln!("✗ {program} unavailable");
        }
    }
    for variable in ["DATABASE_URL", "NOPAGER_MASTER_KEY", "NOPAGER_ADMIN_TOKEN"] {
        if std::env::var(variable).is_ok_and(|value| !value.trim().is_empty()) {
            println!("✓ {variable} is configured");
        } else {
            healthy = false;
            eprintln!("✗ {variable} is missing");
        }
    }
    match client().get(api_url("readyz")?).send().await {
        Ok(response) if response.status().is_success() => println!("✓ NoPager server reachable"),
        _ => {
            healthy = false;
            eprintln!("✗ NoPager server unreachable");
        }
    }
    if !healthy {
        anyhow::bail!("one or more checks failed");
    }
    println!("NoPager is ready to protect an app.");
    Ok(())
}

async fn print_get(path: &str) -> anyhow::Result<()> {
    let response = client().get(api_url(path)?).send().await?;
    ensure_success(response.status())?;
    let value: Value = response.json().await?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

async fn mutate(path: &str) -> anyhow::Result<()> {
    let token = std::env::var("NOPAGER_ADMIN_TOKEN")
        .map_err(|_| anyhow::anyhow!("NOPAGER_ADMIN_TOKEN is required"))?;
    let response = client()
        .post(api_url(path)?)
        .bearer_auth(token)
        .send()
        .await?;
    ensure_success(response.status())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response.json::<Value>().await?)?
    );
    Ok(())
}

fn logs() -> anyhow::Result<()> {
    let status = ProcessCommand::new("docker")
        .args(["compose", "logs", "--follow"])
        .status()?;
    if !status.success() {
        anyhow::bail!("docker compose logs failed");
    }
    Ok(())
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}
fn api_url(path: &str) -> anyhow::Result<String> {
    Ok(format!(
        "{}{}",
        std::env::var("NOPAGER_API_URL").unwrap_or_else(|_| "http://localhost:8080/".into()),
        path
    ))
}
fn web_url() -> String {
    std::env::var("NOPAGER_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:3000".into())
        .trim_end_matches('/')
        .to_owned()
}
fn ensure_success(status: StatusCode) -> anyhow::Result<()> {
    if status.is_success() {
        Ok(())
    } else {
        anyhow::bail!("NoPager API returned {status}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_fills_blank_secrets_without_replacing_existing_values() {
        let (contents, changed) = fill_blank_env(
            "NOPAGER_MASTER_KEY=\nNOPAGER_ADMIN_TOKEN=keep-me\n".into(),
            "NOPAGER_MASTER_KEY",
            "generated",
        );
        assert!(changed);
        assert!(contents.contains("NOPAGER_MASTER_KEY=generated"));
        let (contents, changed) = fill_blank_env(contents, "NOPAGER_ADMIN_TOKEN", "replacement");
        assert!(!changed);
        assert!(contents.contains("NOPAGER_ADMIN_TOKEN=keep-me"));
        assert!(!contents.contains("replacement"));
    }
}
