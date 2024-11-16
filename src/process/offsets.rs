use crate::constant::{self, CLIENT_LIB, ENGINE_LIB, TIER0_LIB};

use super::{memory::Address, process::ProcessHandle};
use anyhow::{Context, Result};
use log::info;

#[derive(Debug, Default)]
pub struct Offsets {
    pub interface: InterfaceOffsets,
    pub library: LibraryOffsets,
    pub direct: DirectOffsets,
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
    pub convar: Address,
    pub player: Address,
}

#[derive(Debug, Default)]
pub struct DirectOffsets {
    pub local_controller: Address,
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

        // Get local controller
        offsets.direct.set_offsets(&offsets.library, process)?;

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
        self.resource = process
            .get_interface_offset(library_offsets.engine.into(), "GameResourceServiceClientV0")?
            .context("Not able to find offset")?
            .into();

        let resource_u64: u64 = self.resource.into();
        let player_address_ptr = resource_u64 + constant::ENTITY_OFFSET;

        let entity_ptr = process.read_u64(player_address_ptr)?;

        self.entity = entity_ptr.into();
        self.player = (entity_ptr + 0x10).into();

        let convar_offset = process
            .get_interface_offset(library_offsets.tier0.into(), "VEngineCvar0")?
            .context("Unable to read cvar offset")?;
        self.convar = convar_offset.into();


        // TODO Figure out netvars



        Ok(())
    }
}

impl DirectOffsets {
    pub fn set_offsets(
        &mut self,
        library_offsets: &LibraryOffsets,
        process: &ProcessHandle,
    ) -> Result<()> {
        let direct_address_ptr = process
            .scan_pattern(
                &[
                    0x48, 0x83, 0x3D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x95, 0xC0, 0xC3,
                ],
                "xxx????xxxxx".as_bytes(),
                library_offsets.client.into(),
            )?
            .context("Unable to find local player controller")?
            .into();

        self.local_controller = process
            .get_relative_address(direct_address_ptr, 0x03, 0x08)?
            .into();

        Ok(())
    }
}
