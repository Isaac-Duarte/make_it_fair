use std::{fmt::Display, path::Path};

use anyhow::{bail, Context, Result};

pub struct Pid(pub(crate) u64);

impl Pid {
    /// Looks for a pid inside of procfs to see if
    pub async fn from_process_name(process_name: &str) -> Result<Pid> {
        let mut reader = tokio::fs::read_dir("/proc").await?;

        while let Some(entry) = reader.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }

            // Try to see if directory in /proc has a u64 as the name
            let file_name = entry.file_name();
            let pid = match file_name
                .to_str()
                .context("Unable to convert to string")?
                .parse::<u64>()
            {
                Ok(pid) => pid,
                Err(_) => continue,
            };

            // exe links to the binary path
            let binary_path = match tokio::fs::read_link(format!("/proc/{}/exe", pid)).await {
                Ok(path) => path,
                Err(_) => continue,
            };

            let binary_name = binary_path
                .file_name()
                .context("Unable to convert OsString to String")?;

            if binary_name == process_name {
                return Ok(Pid(pid));
            }
        }

        bail!("Unable to find process")
    }

    // Validates PID exists
    pub fn validate(&self) -> bool {
        return Path::new(format!("/proc/{}", self.0).as_str()).exists();
    }
}

impl Display for Pid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod test {
    use crate::constant;

    use super::Pid;

    /// Requires CS2 to be open
    #[tokio::test]
    async fn test_cs2_proc() {
        let pid = Pid::from_process_name(constant::PROCESS_NAME).await;

        assert!(!pid.is_err());
        assert!(pid.unwrap().validate());
    }
}
