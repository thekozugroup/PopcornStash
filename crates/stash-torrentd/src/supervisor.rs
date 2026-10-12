//! Root supervisor: VPN bring-up, kill switch, DNS, engine process lifecycle.

use crate::SupervisorOptions;

pub async fn run(options: SupervisorOptions) -> anyhow::Result<()> {
    let _ = options;
    todo!("torrentd agent")
}
