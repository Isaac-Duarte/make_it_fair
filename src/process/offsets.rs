use crate::constant::{self, CLIENT_LIB, ENGINE_LIB, TIER0_LIB};

use super::{
    memory::{self, Address},
    process::ProcessHandle,
};
use anyhow::{Context, Result};
use log::info;

#[derive(Debug, Default)]
pub struct Offsets {
    pub interface: InterfaceOffsets,
    pub library: LibraryOffsets,
    pub direct: DirectOffsets,

    pub network: NetVarOffsets,
}

#[derive(Debug, Default)]
pub struct NetVarOffsets {
    pub controller: PlayerControllerOffsets,
    pub pawn: PawnOffsets,
    pub weapon_service: WeaponServiceOffsets,
    pub money_service: MoneyServiceOffsets,
    pub observer_service: ObserverServiceOffsets,
    pub item_service: ItemServiceOffsets,
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
    pub fn find_offsets(process: &ProcessHandle) -> Result<Offsets> {
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

        // Get Net Var Offsets
        offsets.network.set_offsets(&offsets.library, process)?;

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

impl NetVarOffsets {
    pub fn set_offsets(
        &mut self,
        library_offsets: &LibraryOffsets,
        process: &ProcessHandle,
    ) -> Result<()> {
        let base: u64 = library_offsets.client.into();

        let client_module_size = process.module_size(library_offsets.client.into())?;
        let client_dump = process.dump_module(library_offsets.client.into())?;

        for i in (0..=(client_module_size - 8)).rev().step_by(8) {
            let mut network_enable = false;

            let mut name_pointer = memory::read_u64_vec(&client_dump, i);
            if name_pointer >= base && name_pointer <= base + client_module_size {
                name_pointer = memory::read_u64_vec(&client_dump, name_pointer - base);
                if name_pointer >= base && name_pointer <= base + client_module_size {
                    let name = memory::read_string_vec(&client_dump, name_pointer - base);
                    if name.to_lowercase() == "MNetworkEnable".to_lowercase() {
                        network_enable = true;
                    }
                }
            }

            let name_ptr = match network_enable {
                true => memory::read_u64_vec(&client_dump, i + 0x08),
                false => memory::read_u64_vec(&client_dump, i),
            };

            if name_ptr < base || name_ptr > base + client_module_size {
                continue;
            }

            let netvar_name = memory::read_string_vec(&client_dump, name_ptr - base);

            match netvar_name.as_str() {
                "m_sSanitizedPlayerName" => {
                    if !network_enable || self.controller.m_iszPlayerName.is_valid() {
                        continue;
                    }

                    self.controller.m_iszPlayerName =
                        memory::read_u32_vec(&client_dump, i + 0x08 + 0x10);
                }
                "m_hPawn" => {
                    if !network_enable || self.controller.m_hPawn.is_valid() {
                        continue;
                    }

                    self.controller.m_hPawn = memory::read_u32_vec(&client_dump, i + 0x08 + 0x10);
                }
                "m_iCompTeammateColor" => {
                    if self.controller.m_hPawn.is_valid() {
                        continue;
                    }

                    self.controller.m_iCompTeammateColor =
                        memory::read_u32_vec(&client_dump, i + 0x10);
                }
                "m_iPing" => {
                    if !network_enable || self.controller.m_iPing.is_valid() {
                        continue;
                    }

                    self.controller.m_iPing = memory::read_u32_vec(&client_dump, i + 0x08 + 0x10);
                }
                "m_pInGameMoneyServices" => {
                    if self.controller.m_pInGameMoneyServices.is_valid() {
                        continue;
                    }

                    self.controller.m_pInGameMoneyServices =
                        memory::read_u32_vec(&client_dump, i + 0x10);
                }
                "m_steamID" => {
                    if !network_enable || self.controller.m_steamID.is_valid() {
                        continue;
                    }
                    self.controller.m_steamID = memory::read_u32_vec(&client_dump, i + 0x08 + 0x10);
                }
                "m_iHealth" => {
                    if !network_enable || self.pawn.m_iHealth.is_valid() {
                        continue;
                    }
                    self.pawn.m_iHealth = memory::read_u32_vec(&client_dump, i + 0x08 + 0x10);
                }
                "m_ArmorValue" => {
                    if !network_enable || self.pawn.m_ArmorValue.is_valid() {
                        continue;
                    }
                    self.pawn.m_ArmorValue = memory::read_u32_vec(&client_dump, i + 0x08 + 0x10);
                }
                "m_iTeamNum" => {
                    if !network_enable || self.pawn.m_iTeamNum.is_valid() {
                        continue;
                    }

                    self.pawn.m_iTeamNum = memory::read_u32_vec(&client_dump, i + 0x08 + 0x10);
                }
                "m_lifeState" => {
                    if !network_enable || self.pawn.m_lifeState.is_valid() {
                        continue;
                    }

                    self.pawn.m_lifeState = memory::read_u32_vec(&client_dump, i + 0x08 + 0x10);
                }
                "m_pClippingWeapon" => {
                    if self.pawn.m_pClippingWeapon.is_valid() {
                        continue;
                    }

                    self.pawn.m_pClippingWeapon = memory::read_u32_vec(&client_dump, i + 0x10);
                }
                "m_vOldOrigin" => {
                    if self.pawn.m_vOldOrigin.is_valid() {
                        continue;
                    }

                    self.pawn.m_vOldOrigin = memory::read_u32_vec(&client_dump, i + 0x08);
                }
                "m_angEyeAngles" => {
                    if self.pawn.m_angEyeAngles.is_valid() {
                        continue;
                    }

                    self.pawn.m_angEyeAngles = memory::read_u32_vec(&client_dump, i + 0x10);
                }
                "m_pWeaponServices" => {
                    if self.pawn.m_pWeaponServices.is_valid() {
                        continue;
                    }

                    self.pawn.m_pWeaponServices = memory::read_u32_vec(&client_dump, i + 0x08);
                }
                "m_pObserverServices" => {
                    if self.pawn.m_pObserverServices.is_valid() {
                        continue;
                    }

                    self.pawn.m_pObserverServices = memory::read_u32_vec(&client_dump, i + 0x08);
                }
                "m_pItemServices" => {
                    if self.pawn.m_pItemServices.is_valid() {
                        continue;
                    }

                    self.pawn.m_pItemServices = memory::read_u32_vec(&client_dump, i + 0x08);
                }
                "m_hActiveWeapon" => {
                    if !network_enable || self.weapon_service.m_hActiveWeapon.is_valid() {
                        continue;
                    }

                    self.weapon_service.m_hActiveWeapon =
                        memory::read_u32_vec(&client_dump, i + 0x08 + 0x10);
                }
                "m_hMyWeapons" => {
                    if self.weapon_service.m_hMyWeapons.is_valid() {
                        continue;
                    }

                    self.weapon_service.m_hMyWeapons = memory::read_u32_vec(&client_dump, i + 0x08);
                }
                "m_iAccount" => {
                    if self.money_service.m_iAccount.is_valid() {
                        continue;
                    }

                    self.money_service.m_iAccount = memory::read_u32_vec(&client_dump, i + 0x10);
                }
                "m_hObserverTarget" => {
                    if self.observer_service.m_hObserverTarget.is_valid() {
                        continue;
                    }
                    self.observer_service.m_hObserverTarget =
                        memory::read_u32_vec(&client_dump, i + 0x08);
                }
                "m_bHasDefuser" => {
                    if self.item_service.m_bHasDefuser.is_valid() {
                        continue;
                    }
                    self.item_service.m_bHasDefuser = memory::read_u32_vec(&client_dump, i + 0x10);
                }
                "m_bHasHelmet" => {
                    if !network_enable || self.item_service.m_bHasHelmet.is_valid() {
                        continue;
                    }

                    self.item_service.m_bHasHelmet =
                        memory::read_u32_vec(&client_dump, i + 0x08 + 0x10);
                }
                _ => {}
            }
        }

        Ok(())
    }
}

#[allow(non_snake_case)]
#[derive(Debug, Default)]
pub struct PlayerControllerOffsets {
    pub m_iszPlayerName: Address,        // string (m_iszPlayerName)
    pub m_hPawn: Address,                // pointer -> Pawn (m_hPawn)
    pub m_iCompTeammateColor: Address,   // i32 (m_iCompTeammateColor)
    pub m_iPing: Address,                // i32 (m_iPing)
    pub m_pInGameMoneyServices: Address, // pointer -> MoneyServices (m_pInGameMoneyServices)
    pub m_steamID: Address,              // u64 (m_steamID)
}

#[allow(non_snake_case)]
#[derive(Debug, Default)]
pub struct PawnOffsets {
    pub m_iHealth: Address,           // i32 (m_iHealth)
    pub m_ArmorValue: Address,        // i32 (m_ArmorValue)
    pub m_iTeamNum: Address,          // i32 (m_iTeamNum)
    pub m_lifeState: Address,         // i32 (m_lifeState)
    pub m_pClippingWeapon: Address,   // pointer -> WeaponBase (m_pClippingWeapon)
    pub m_vOldOrigin: Address,        // vec3 (m_vOldOrigin)
    pub m_angEyeAngles: Address,      // vec3? (m_angEyeAngles)
    pub m_pWeaponServices: Address,   // pointer -> WeaponServices (m_pWeaponServices)
    pub m_pObserverServices: Address, // pointer -> ObserverServices (m_pObserverServices)
    pub m_pItemServices: Address,     // pointer -> ItemServices (m_pItemServices)
}

#[allow(non_snake_case)]
#[derive(Debug, Default)]
pub struct WeaponServiceOffsets {
    pub m_hActiveWeapon: Address, // pointer -> Weapon (m_hActiveWeapon)
    pub m_hMyWeapons: Address,    // pointer -> Vec<pointer -> Weapon> (m_hMyWeapons)
}

#[allow(non_snake_case)]
#[derive(Debug, Default)]
pub struct MoneyServiceOffsets {
    pub m_iAccount: Address, // i32 (m_iAccount)
}

#[allow(non_snake_case)]
#[derive(Debug, Default)]
pub struct ObserverServiceOffsets {
    pub m_hObserverTarget: Address, // pointer -> Pawn (m_hObserverTarget)
}

#[allow(non_snake_case)]
#[derive(Debug, Default)]
pub struct ItemServiceOffsets {
    pub m_bHasDefuser: Address, // bool (m_bHasDefuser)
    pub m_bHasHelmet: Address,  // bool (m_bHasHelmet)
}
