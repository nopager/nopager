#[cfg(target_os = "linux")]
mod linux {
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc, time::Duration};

    use clap::Parser;
    use nopager_runtime_helper::{DockerCliBackend, HelperConfig, RuntimeExecutor};
    use nopager_runtime_protocol::{
        MAX_MESSAGE_BYTES, MutationDisposition, RuntimeErrorCode, RuntimeRequest, RuntimeResponse,
    };
    use secrecy::SecretString;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{UnixListener, UnixStream},
        sync::Semaphore,
    };
    use tracing::{error, info, warn};
    use tracing_subscriber::EnvFilter;
    use uuid::Uuid;

    const MAX_CONCURRENT_CONNECTIONS: usize = 16;
    const CONNECTION_LIFETIME: Duration = Duration::from_secs(65);

    #[derive(Debug, Parser)]
    #[command(version, about = "NoPager deterministic host runtime helper")]
    struct Args {
        #[arg(long, default_value = "/etc/nopager/runtime-helper.json")]
        config: PathBuf,
        #[arg(long, default_value = "/etc/nopager/runtime-helper.token")]
        credential_file: PathBuf,
        #[arg(long, default_value = "/run/nopager-runtime/runtime-helper.sock")]
        socket: PathBuf,
        #[arg(long, default_value = "/var/lib/nopager-runtime/requests")]
        state_directory: PathBuf,
        #[arg(long, default_value = "/usr/bin/docker")]
        docker_program: PathBuf,
    }

    pub async fn run() -> anyhow::Result<()> {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .json()
            .init();
        let args = Args::parse();
        require_absolute_paths(&args)?;

        let config: HelperConfig = serde_json::from_slice(&fs::read(&args.config)?)?;
        let credential = fs::read_to_string(&args.credential_file)?.trim().to_owned();
        let backend = Arc::new(DockerCliBackend::new(args.docker_program)?);
        let executor = Arc::new(RuntimeExecutor::new(
            config,
            &SecretString::from(credential),
            args.state_directory,
            backend,
        )?);
        let enrolled = executor.validate_startup().await?;

        if let Some(parent) = args.socket.parent() {
            fs::create_dir_all(parent)?;
        }
        if args.socket.exists() {
            fs::remove_file(&args.socket)?;
        }
        let listener = UnixListener::bind(&args.socket)?;
        fs::set_permissions(&args.socket, fs::Permissions::from_mode(0o660))?;
        info!(
            socket = %args.socket.display(),
            target_id = %enrolled.id,
            target_name = %enrolled.name,
            "runtime helper started with one immutable enrolled target"
        );
        let connections = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));

        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, _)) => {
                            let Ok(permit) = connections.clone().try_acquire_owned() else {
                                warn!("runtime helper connection limit reached");
                                continue;
                            };
                            let executor = executor.clone();
                            tokio::spawn(async move {
                                let _permit = permit;
                                match tokio::time::timeout(
                                    CONNECTION_LIFETIME,
                                    serve_connection(stream, executor),
                                )
                                .await
                                {
                                    Ok(Ok(())) => {}
                                    Ok(Err(error)) => {
                                        warn!(%error, "runtime helper rejected an IPC connection");
                                    }
                                    Err(_) => warn!("runtime helper connection timed out"),
                                }
                            });
                        }
                        Err(error) => error!(%error, "runtime helper accept failed"),
                    }
                }
                _ = tokio::signal::ctrl_c() => break,
            }
        }
        let _ = fs::remove_file(args.socket);
        Ok(())
    }

    async fn serve_connection(
        mut stream: UnixStream,
        executor: Arc<RuntimeExecutor>,
    ) -> anyhow::Result<()> {
        let peer_uid = stream.peer_cred()?.uid();
        let mut input = Vec::new();
        let mut limited = (&mut stream).take((MAX_MESSAGE_BYTES + 1) as u64);
        limited.read_to_end(&mut input).await?;
        let response = if input.len() > MAX_MESSAGE_BYTES {
            RuntimeResponse::rejection(
                Uuid::nil(),
                RuntimeErrorCode::InvalidRequest,
                MutationDisposition::NotStarted,
                "request exceeded the fixed protocol size limit",
            )
        } else {
            match serde_json::from_slice::<RuntimeRequest>(&input) {
                Ok(request) => executor.handle(peer_uid, request).await,
                Err(_) => RuntimeResponse::rejection(
                    Uuid::nil(),
                    RuntimeErrorCode::InvalidRequest,
                    MutationDisposition::NotStarted,
                    "request was not valid protocol JSON",
                ),
            }
        };
        let mut output = serde_json::to_vec(&response)?;
        output.push(b'\n');
        stream.write_all(&output).await?;
        stream.shutdown().await?;
        Ok(())
    }

    fn require_absolute_paths(args: &Args) -> anyhow::Result<()> {
        for path in [
            &args.config,
            &args.credential_file,
            &args.socket,
            &args.state_directory,
            &args.docker_program,
        ] {
            if !path.is_absolute() {
                anyhow::bail!("runtime helper paths must be absolute: {}", path.display());
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    linux::run().await
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("nopager-runtime-helper is supported only on Linux");
    std::process::exit(1);
}
