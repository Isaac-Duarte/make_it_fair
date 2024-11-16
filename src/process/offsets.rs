use crate::constant::{CLIENT_LIB, ENGINE_LIB, TIER0_LIB};

use super::{memory::Address, process::ProcessHandle};
use anyhow::{Context, Result};
use log::info;

#[derive(Debug, Default)]
pub struct Offsets {
    interface: InterfaceOffsets,
    library: LibraryOffsets,
}

#[derive(Debug, Default)]
pub struct LibraryOffsets {
    // libclient.so
    pub client: Address,

    // libengine2.so
    pub engine: Address,

    // libteir0.so
    pub tier0: Address,
}

#[derive(Debug, Default)]
pub struct InterfaceOffsets {
    pub resource: Address,
    pub entity: Address,
    pub cvar: Address,
    pub player: Address,
}

impl Offsets {
    pub async fn find_offsets(process: &ProcessHandle) -> Result<Offsets> {
        let mut offsets = Offsets::default();

        // Set Shared Object offsets
        offsets
            .library
            .set_offsets(process)
            .context("Unable to set Library Offsets")?;

        // Set Interface Offsets
        offsets.interface.set_offsets(&offsets.library, process)?;

        Ok(offsets)
    }
}

impl LibraryOffsets {
    pub fn set_offsets(&mut self, process: &ProcessHandle) -> Result<()> {
        self.client = process.get_module_base_address(CLIENT_LIB)?;
        self.engine = process.get_module_base_address(ENGINE_LIB)?;
        self.tier0 = process.get_module_base_address(TIER0_LIB)?;

        Ok(())
    }
}

impl InterfaceOffsets {
    pub fn set_offsets(
        &mut self,
        library_offsets: &LibraryOffsets,
        process: &ProcessHandle,
    ) -> Result<()> {
        self.resource = Address::from(
            process
                .get_interface_offset(library_offsets.engine.into(), "GameResourceServiceClientV0")?
                .context("Not able to find offset")?,
        );

        Ok(())
    }
}
