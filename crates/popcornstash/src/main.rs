//! `popcornstash`: one binary, several roles (docs/architecture.md section 1).

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "popcornstash", version, about = "A private, organized home for your family's videos and photos")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the web server, library and downloads (container 1).
    Serve,
    /// Apply the VPN and kill switch, then supervise the torrent engine (container 2, root).
    Torrentd {
        #[arg(long, env = "STASH_TORRENT_DATA", default_value = "/data")]
        data_dir: PathBuf,
        #[arg(long, env = "STASH_DOWNLOADS", default_value = "/stash/Downloads")]
        downloads_dir: PathBuf,
        #[arg(long, env = "PUID", default_value_t = 99)]
        puid: u32,
        #[arg(long, env = "PGID", default_value_t = 100)]
        pgid: u32,
        #[arg(long, env = "STASH_ALLOW_NO_VPN", default_value_t = false)]
        allow_no_vpn: bool,
    },
    /// The unprivileged torrent engine. Started by `torrentd`; not for direct use.
    #[command(hide = true)]
    TorrentEngine {
        #[arg(long, env = "STASH_TORRENT_DATA", default_value = "/data")]
        data_dir: PathBuf,
        #[arg(long, env = "STASH_DOWNLOADS", default_value = "/stash/Downloads")]
        downloads_dir: PathBuf,
        #[arg(long, env = "STASH_TUNNEL_IF")]
        tunnel_if: Option<String>,
        #[arg(long, env = "STASH_ALLOW_NO_VPN", default_value_t = false)]
        allow_no_vpn: bool,
        #[arg(long, default_value = "0.0.0.0:9091")]
        api_bind: SocketAddr,
        #[arg(long, default_value = "0.0.0.0:9092")]
        proxy_bind: SocketAddr,
    },
    /// Maintenance commands, run with `docker exec`.
    Admin {
        #[command(subcommand)]
        command: AdminCommand,
    },
    /// Exit 0 when the local server (or engine with --engine) answers its health check.
    Healthcheck {
        #[arg(long)]
        engine: bool,
    },
}

#[derive(Subcommand)]
enum AdminCommand {
    /// Print a one-time link that lets the owner register a new passkey (valid 30 minutes).
    ResetOwner,
    /// Print the current first-run setup code (only while no owner exists).
    SetupCode,
}

fn init_tracing() {
    let filter = EnvFilter::try_from_env("STASH_LOG").unwrap_or_else(|_| EnvFilter::new("info,librqbit=warn,sqlx=warn,tower_http=info"));
    tracing_subscriber::fmt().with_env_filter(filter).with_target(false).init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve => {
            init_tracing();
            let config = stash_core::config::Config::from_env()?;
            stash_server::serve(config).await
        }
        Command::Torrentd { data_dir, downloads_dir, puid, pgid, allow_no_vpn } => {
            init_tracing();
            stash_torrentd::run_supervisor(stash_torrentd::SupervisorOptions {
                data_dir,
                downloads_dir,
                puid,
                pgid,
                allow_no_vpn,
                engine_exe: std::env::current_exe()?,
            })
            .await
        }
        Command::TorrentEngine { data_dir, downloads_dir, tunnel_if, allow_no_vpn, api_bind, proxy_bind } => {
            init_tracing();
            let code = stash_torrentd::run_engine(stash_torrentd::EngineOptions {
                data_dir,
                downloads_dir,
                tunnel_if,
                allow_no_vpn,
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
                    println!("Open this link within 30 minutes to add a new passkey for the owner:\n{link}");
                }
                AdminCommand::SetupCode => {
                    let auth = stash_server::auth::AuthService::new(std::sync::Arc::new(config), db)?;
                    match auth.ensure_setup_code().await.map_err(|e| anyhow::anyhow!("{e}"))? {
                        Some(code) => println!("Setup code: {code}"),
                        None => println!("An owner already exists. Use `popcornstash admin reset-owner` to recover access."),
                    }
                }
            }
            Ok(())
        }
        Command::Healthcheck { engine } => healthcheck(engine).await,
    }
}

async fn healthcheck(engine: bool) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (addr, path) = if engine {
        ("127.0.0.1:9091".to_string(), "/livez")
    } else {
        let bind = std::env::var("STASH_BIND").unwrap_or_else(|_| "0.0.0.0:8788".into());
        let port = bind.rsplit(':').next().unwrap_or("8788").to_string();
        (format!("127.0.0.1:{port}"), "/api/health")
    };
    let mut stream = tokio::time::timeout(std::time::Duration::from_secs(3), tokio::net::TcpStream::connect(&addr)).await??;
    stream
        .write_all(format!("GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").as_bytes())
        .await?;
    let mut buf = vec![0u8; 64];
    let n = tokio::time::timeout(std::time::Duration::from_secs(3), stream.read(&mut buf)).await??;
    let head = String::from_utf8_lossy(&buf[..n]);
    if head.starts_with("HTTP/1.0 200") || head.starts_with("HTTP/1.1 200") {
        Ok(())
    } else {
        anyhow::bail!("unhealthy: {}", head.lines().next().unwrap_or(""))
    }
}
