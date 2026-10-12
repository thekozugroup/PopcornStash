//! Client for the supervisor's adopt socket (docs/architecture.md section 5, step 9).

use std::path::Path;
use std::time::Duration;

use stash_core::config::Config;
use stash_core::internal_api::{SupervisorRequest, SupervisorResponse};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const TIMEOUT: Duration = Duration::from_secs(120);

/// Ask the supervisor to re-own `Downloads/In Progress/<name>` as PUID:PGID.
/// A no-op when `serve` runs without the supervisor (development).
pub async fn adopt(config: &Config, name: &str) -> anyhow::Result<()> {
    let Some(socket) = config.supervisor_socket.as_deref() else {
        return Ok(());
    };
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        anyhow::bail!("refusing to adopt {name:?}: not a single path component");
    }
    tokio::time::timeout(TIMEOUT, request(socket, &SupervisorRequest::Adopt { name: name.to_string() }))
        .await
        .map_err(|_| anyhow::anyhow!("the supervisor did not answer the adopt request in time"))?
}

async fn request(socket: &Path, request: &SupervisorRequest) -> anyhow::Result<()> {
    let stream = UnixStream::connect(socket).await?;
    let (read, mut write) = stream.into_split();
    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    write.write_all(line.as_bytes()).await?;
    write.shutdown().await?;
    let mut reader = BufReader::new(read);
    let mut answer = String::new();
    reader.read_line(&mut answer).await?;
    let response: SupervisorResponse = serde_json::from_str(answer.trim())?;
    if response.ok {
        Ok(())
    } else {
        anyhow::bail!(response.error.unwrap_or_else(|| "the supervisor refused the request".into()))
    }
}
