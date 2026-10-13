//! `popcornstash`: one binary, several roles (docs/architecture.md section 1).

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "popcornstash",
    version,
    about = "A private, organized home for your family's videos and photos"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start everything (default in the container): the root supervisor applies the VPN
    /// and kill switch, then runs the web server and torrent engine with dropped privileges.
    Run {
        #[arg(long, env = "PUID", default_value_t = 99)]
        puid: u32,
        #[arg(long, env = "PGID", default_value_t = 100)]
        pgid: u32,
        #[arg(long, env = "STASH_TORRENT_UID", default_value_t = 6881)]
        engine_uid: u32,
        #[arg(long, env = "STASH_ALLOW_NO_VPN", default_value_t = false)]
        allow_no_vpn: bool,
    },
    /// Run only the web server, library and downloads (development, or started by `run`).
    Serve,
    /// The unprivileged torrent engine. Started by `run`; not for direct use.
    #[command(hide = true)]
    TorrentEngine {
        #[arg(long, env = "STASH_ENGINE_STATE")]
        state_dir: PathBuf,
        #[arg(long, env = "STASH_ENGINE_DOWNLOADS")]
        downloads_dir: PathBuf,
        #[arg(
            long,
            env = "STASH_ENGINE_STATUS",
            default_value = "/run/popcornstash/status.json"
        )]
        status_file: PathBuf,
        #[arg(long, env = "STASH_TUNNEL_IF")]
        tunnel_if: Option<String>,
        #[arg(long, env = "STASH_ALLOW_NO_VPN", default_value_t = false)]
        allow_no_vpn: bool,
        #[arg(long, default_value = stash_core::internal_api::ENGINE_API_ADDR)]
        api_bind: SocketAddr,
        #[arg(long, default_value = stash_core::internal_api::ENGINE_PROXY_ADDR)]
        proxy_bind: SocketAddr,
    },
    /// Maintenance commands, run with `docker exec`.
    Admin {
        #[command(subcommand)]
        command: AdminCommand,
    },
    /// Exit 0 when the web server answers its health check.
    Healthcheck,
}

#[derive(Subcommand)]
enum AdminCommand {
    /// Print a one-time link that lets the owner register a new passkey (valid 30 minutes).
    ResetOwner,
    /// Print a new first-run setup code (only while no owner exists). The code in the log stops working.
    SetupCode,
}

fn init_tracing() {
    let filter = EnvFilter::try_from_env("STASH_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,librqbit=warn,sqlx=warn,tower_http=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run {
            puid,
            pgid,
            engine_uid,
            allow_no_vpn,
        } => {
            init_tracing();
            let config = stash_core::config::Config::from_env()?;
            stash_torrentd::run_supervisor(stash_torrentd::SupervisorOptions {
                config_dir: config.config_dir.clone(),
                stash_root: config.stash_root.clone(),
                puid,
                pgid,
                engine_uid,
                allow_no_vpn,
                exe: std::env::current_exe()?,
                run_dir: PathBuf::from(stash_core::internal_api::RUN_DIR),
            })
            .await
        }
        Command::Serve => {
            init_tracing();
            let config = stash_core::config::Config::from_env()?;
            stash_server::serve(config).await
        }
        Command::TorrentEngine {
            state_dir,
            downloads_dir,
            status_file,
            tunnel_if,
            allow_no_vpn,
            api_bind,
            proxy_bind,
        } => {
            init_tracing();
            let internal_token = std::env::var("STASH_INTERNAL_TOKEN").map_err(|_| {
                anyhow::anyhow!(
                    "STASH_INTERNAL_TOKEN is not set; the engine is started by `popcornstash run`"
                )
            })?;
            let code = stash_torrentd::run_engine(stash_torrentd::EngineOptions {
                state_dir,
                downloads_dir,
                status_file,
                tunnel_if,
                allow_no_vpn,
                internal_token,
                api_bind,
                proxy_bind,
            })
            .await?;
            std::process::exit(code)
        }
        Command::Admin { command } => {
            let config = stash_core::config::Config::from_env()?;
            let db = stash_core::db::open(&config.db_path()).await?;
            match command {
                AdminCommand::ResetOwner => {
                    let link = stash_server::auth::create_owner_reset_link(&db, &config).await?;
                    println!(
                        "Open this link within 30 minutes to add a new passkey for the owner:\n{link}"
                    );
                }
                AdminCommand::SetupCode => {
                    let auth =
                        stash_server::auth::AuthService::new(std::sync::Arc::new(config), db)?;
                    match auth
                        .ensure_setup_code()
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                    {
                        Some(code) => println!(
                            "Setup code: {code}\nThis new code is valid for 24 hours. The code in the server log no longer works."
                        ),
                        None => println!(
                            "An owner already exists. Use `popcornstash admin reset-owner` to recover access."
                        ),
                    }
                }
            }
            Ok(())
        }
        Command::Healthcheck => healthcheck().await,
    }
}

async fn healthcheck() -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let bind = std::env::var("STASH_BIND").unwrap_or_else(|_| "0.0.0.0:8788".into());
    let port = bind.rsplit(':').next().unwrap_or("8788").to_string();
    let tls = std::env::var("STASH_TLS_CERT").is_ok_and(|v| !v.trim().is_empty());
    if tls {
        // Built-in HTTPS: the certificate names the public host, not 127.0.0.1, so only
        // this loopback health probe skips certificate checks.
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(3))
            .build()?;
        let response = client
            .get(format!("https://127.0.0.1:{port}/api/health"))
            .send()
            .await?;
        if response.status().is_success() {
            return Ok(());
        }
        anyhow::bail!("unhealthy: HTTP {}", response.status());
    }
    let addr = format!("127.0.0.1:{port}");
    let path = "/api/health";
    let mut stream = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::net::TcpStream::connect(&addr),
    )
    .await??;
    stream
        .write_all(format!("GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").as_bytes())
        .await?;
    let mut buf = vec![0u8; 64];
    let n =
        tokio::time::timeout(std::time::Duration::from_secs(3), stream.read(&mut buf)).await??;
    let head = String::from_utf8_lossy(&buf[..n]);
    if head.starts_with("HTTP/1.0 200") || head.starts_with("HTTP/1.1 200") {
        Ok(())
    } else {
        anyhow::bail!("unhealthy: {}", head.lines().next().unwrap_or(""))
    }
}
