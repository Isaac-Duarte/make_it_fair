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

// fn get_pawn_name(process: &ProcessHandle, controller: u64) -> String {
//     let name_pointer = process
//         .read_u64(controller + cs2dumper::client::CBasePlayerController::m_iszPlayerName as u64);

//     if name_pointer == 0 {
//         return String::from("?");
//     }

//     process.read_string(name_pointer)
// }