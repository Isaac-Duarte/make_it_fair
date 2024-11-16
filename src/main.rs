use anyhow::Result;
use log::info;
use process::{offsets::Offsets, pid::Pid};

pub mod constant;
pub mod model;
pub mod process;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    pretty_env_logger::init();

    let process = process::process::ProcessHandle::from_pid(
        Pid::from_process_name(constant::PROCESS_NAME).await?,
    )
    .await?;

    let offsets = Offsets::find_offsets(&process).await?;

    info!("{:?}", offsets);

    Ok(())
}
