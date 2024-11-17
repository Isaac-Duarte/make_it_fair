use anyhow::Result;
use cs2_interface::Cs2Interface;
use log::info;
use process::{offsets::Offsets, pid::Pid};

pub(crate) mod constant;
pub mod process; 
pub mod cs2_interface;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    pretty_env_logger::init();

    let process = process::process::ProcessHandle::from_pid(
        Pid::from_process_name(constant::PROCESS_NAME).await?,
    )
    .await?;

    let interface = Cs2Interface::new(process)?;
    
    Ok(())
}
