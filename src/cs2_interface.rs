use anyhow::{bail, Context, Result};
use log::info;
use serde::Serialize;
use std::collections::HashMap;

use crate::process::{memory::Address, offsets::Offsets, process::ProcessHandle};

pub type ControllerAddress = Address;
pub type PawnAddress = Address;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Player {
    pub name: String,
    pub health: i32,
    pub armor: i32,
    pub money: i32,
    pub team: Team,
    pub life_state: LifeState,
    pub weapon: String,
    pub weapons: Vec<String>,
    pub has_defuser: bool,
    pub has_helmet: bool,
    pub color: i32,
    pub position: Vec3,
    pub rotation: f32,
    pub ping: i32,
    pub steam_id: u64,
    pub active_player: bool,
    pub is_local_player: bool,
}

#[repr(u8)]
#[derive(Debug, Default, Clone, Serialize)]
pub enum Team {
    #[default]
    Speactator = 1,
    Terrorist = 2,
    CounterTerrorist = 3,
}

#[repr(u8)]
#[derive(Debug, Default, Clone, Serialize)]
pub enum LifeState {
    Alive, // Alive
    Dying, // Playing death animation falling off a ledge
    #[default]
    Dead, // Dead
    Respawnable,
    DiscardBody,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub struct Cs2Interface {
    offsets: Offsets,
    process_handle: ProcessHandle,
    convars: HashMap<String, Address>,
}

impl Cs2Interface {
    pub fn new(process_handle: ProcessHandle) -> Result<Self> {
        let mut interface = Cs2Interface {
            offsets: Offsets::find_offsets(&process_handle)?,
            process_handle,
            convars: HashMap::new(),
        };

        interface.set_convars()?;

        Ok(interface)
    }

    /// Finds all convars and stores them into a map
    fn set_convars(&mut self) -> Result<()> {
        if self.offsets.interface.convar.is_null() {
            bail!("Convar offset has not been set");
        }

        let offset: u64 = self.offsets.interface.convar.into();
        let objects = self.process_handle.read_u64(offset + 0x40)?;

        for i in 0..self.process_handle.read_u64(offset + 0xA0)? {
            let object = self.process_handle.read_u64_address(objects + i * 0x10)?;

            if object.is_null() {
                break;
            }

            let name_address = self.process_handle.read_u64_address(object)?;
            let name = self.process_handle.read_string(name_address)?;

            self.convars.insert(name, object);
        }

        Ok(())
    }

    fn get_local_controller(&self) -> Result<ControllerAddress> {
        self.process_handle
            .read_u64_address(self.offsets.direct.local_controller)
    }

    fn get_pawn(&self, controller: ControllerAddress) -> Result<PawnAddress> {
        let uhandle =
            self.process_handle
                .read_u32(controller + self.offsets.network.controller.m_hPawn)? as u64;

        let list_entry = self.process_handle.read_u64_address(
            self.offsets.interface.player
                + Address::from(0x8) * Address::from((uhandle & 0x7FFF) >> 9),
        )?;

        let pawn = self
            .process_handle
            .read_u64_address(list_entry + Address::from(120) * Address::from(uhandle & 0x1FF))?;

        Ok(pawn)
    }

    /// Gets a players name given the controller address
    fn get_name(&self, controller: ControllerAddress) -> Result<Option<String>> {
        let name_pointer = self
            .process_handle
            .read_u64_address(controller + self.offsets.network.controller.m_iszPlayerName)?;

        if name_pointer.is_null() {
            Ok(None)
        } else {
            Ok(Some(self.process_handle.read_string(name_pointer)?))
        }
    }

    /// Gets a players health given the pawn address
    fn get_health(&self, pawn: PawnAddress) -> Result<i32> {
        let health = self
            .process_handle
            .read_i32(pawn + self.offsets.network.pawn.m_iHealth)?;

        if !(0..=100).contains(&health) {
            return Ok(0);
        }

        Ok(health)
    }

    /// Gets a players health given the pawn address
    fn get_armor(&self, pawn: PawnAddress) -> Result<i32> {
        let armor = self
            .process_handle
            .read_i32(pawn + self.offsets.network.pawn.m_ArmorValue)?;

        if !(0..=100).contains(&armor) {
            return Ok(0);
        }

        Ok(armor)
    }

    /// Gets a players health given the pawn address
    fn get_money(&self, controller: ControllerAddress) -> Result<i32> {
        let money_services = self.process_handle.read_u64_address(
            controller + self.offsets.network.controller.m_pInGameMoneyServices,
        )?;

        if money_services.is_null() {
            return Ok(0);
        }

        let money = self
            .process_handle
            .read_i32(money_services + self.offsets.network.money_service.m_iAccount)?;

        if !(0..=99999).contains(&money) {
            return Ok(0);
        }

        Ok(money)
    }

    /// Gets a players name given the controller address
    fn get_team(&self, pawn: PawnAddress) -> Result<Option<Team>> {
        let team = self
            .process_handle
            .read_u8(pawn + self.offsets.network.pawn.m_iTeamNum)?;

        Ok(match team {
            1 => Some(Team::Speactator),
            2 => Some(Team::Terrorist),
            3 => Some(Team::CounterTerrorist),
            _ => None,
        })
    }

    // Gets the players life state
    fn get_life_state(&self, pawn: PawnAddress) -> Result<Option<LifeState>> {
        let life_state = self
            .process_handle
            .read_u8(pawn + self.offsets.network.pawn.m_lifeState)?;

        Ok(match life_state {
            0 => Some(LifeState::Alive),
            1 => Some(LifeState::Dying),
            2 => Some(LifeState::Dead),
            3 => Some(LifeState::Respawnable),
            4 => Some(LifeState::DiscardBody),
            _ => None,
        })
    }

    // TODO Return Enum
    fn get_weapon(&self, pawn: PawnAddress) -> Result<Option<String>> {
        // CEntityInstance
        let weapon_entity_instance = self
            .process_handle
            .read_u64_address(pawn + self.offsets.network.pawn.m_pClippingWeapon)?;

        if weapon_entity_instance.is_null() {
            return Ok(None);
        }

        self.get_weapon_name(weapon_entity_instance)
    }

    // Gets weapon name from the pointer
    fn get_weapon_name(&self, weapon_instance: Address) -> Result<Option<String>> {
        // CEntityIdentity, 0x10 = m_pEntity
        let weapon_entity_identity = self
            .process_handle
            .read_u64_address(weapon_instance + Address::from(0x10))?;

        if weapon_entity_identity.is_null() {
            return Ok(None);
        }

        // 0x20 = m_designerName (pointer -> string)
        let weapon_name_pointer = self
            .process_handle
            .read_u64_address(weapon_entity_identity + Address::from(0x20))?;

        if weapon_name_pointer.is_null() {
            return Ok(None);
        }

        Ok(Some(self.process_handle.read_string(weapon_name_pointer)?))
    }

    // Gets all weapons given the pawn
    fn get_weapons(&self, pawn: PawnAddress) -> Result<Vec<String>> {
        let weapon_services = self
            .process_handle
            .read_u64_address(pawn + self.offsets.network.pawn.m_pWeaponServices)?;

        if weapon_services.is_null() {
            return Ok(vec![]);
        }

        // 8 bytes size, 8 bytes pointer to data
        let size = self
            .process_handle
            .read_u64(weapon_services + self.offsets.network.weapon_service.m_hMyWeapons)?;

        let weapon_vector = self.process_handle.read_u64(
            weapon_services
                + self.offsets.network.weapon_service.m_hMyWeapons
                + Address::from(0x08),
        )?;

        let mut weapon_names = vec![];

        for i in 0..size {
            // weird bit-fuckery, why exactly does it need the & 0xfff?
            // No idea. - Isaac
            let weapon_index = self.process_handle.read_u32(weapon_vector + i * 0x04)? & 0xfff;

            let weapon_entity = self.get_client_entity(weapon_index as u64)?;

            if let Some(entity) = weapon_entity {
                let weapon_name = self.get_weapon_name(entity)?;

                if let Some(weapon_name) = weapon_name {
                    weapon_names.push(weapon_name);
                }
            }
        }

        Ok(weapon_names)
    }

    fn get_client_entity(&self, index: impl Into<Address>) -> Result<Option<Address>> {
        let index = index.into();

        // wtf is this doing, and how?
        let v1 = self.process_handle.read_u64_address(
            self.offsets.interface.entity
                + Address::from(0x08) * (index >> 9)
                + Address::from(0x10),
        )?;

        if v1.is_null() {
            return Ok(None);
        }

        // what?
        let entity = self
            .process_handle
            .read_u64_address(v1 + Address::from(120) * (index & Address::from(0x1ff)))?;

        if entity.is_null() {
            return Ok(None);
        }

        Ok(Some(entity))
    }

    /// Gets a players health given the pawn address
    fn get_defuser(&self, pawn: PawnAddress) -> Result<bool> {
        let item_services = self
            .process_handle
            .read_u64_address(pawn + self.offsets.network.pawn.m_pItemServices)?;

        if item_services.is_null() {
            return Ok(false);
        }

        Ok(self
            .process_handle
            .read_u8(item_services + self.offsets.network.item_service.m_bHasDefuser)?
            != 0)
    }

    // Returns if a player has a helmet or not
    fn get_helmet(&self, pawn: PawnAddress) -> Result<bool> {
        let item_services = self
            .process_handle
            .read_u64_address(pawn + self.offsets.network.pawn.m_pItemServices)?;

        if item_services.is_null() {
            return Ok(false);
        }

        Ok(self
            .process_handle
            .read_u8(item_services + self.offsets.network.item_service.m_bHasHelmet)?
            != 0)
    }

    fn get_color(&self, controller: ControllerAddress) -> Result<i32> {
        self.process_handle
            .read_i32(controller + self.offsets.network.controller.m_iCompTeammateColor)
    }

    fn get_position(&self, pawn: PawnAddress) -> Result<Vec3> {
        // 3 32-bit floats
        let position = pawn + self.offsets.network.pawn.m_vOldOrigin;

        Ok(Vec3 {
            x: self.process_handle.read_f32(position)?,
            y: self
                .process_handle
                .read_f32(position + Address::from(0x04))?,
            z: self
                .process_handle
                .read_f32(position + Address::from(0x08))?,
        })
    }

    fn get_rotation(&self, pawn: PawnAddress) -> Result<f32> {
        self.process_handle
            .read_f32(pawn + self.offsets.network.pawn.m_angEyeAngles)
    }

    fn get_ping(&self, controller: ControllerAddress) -> Result<i32> {
        self.process_handle
            .read_i32(controller + self.offsets.network.controller.m_iPing)
    }

    fn get_steam_id(&self, controller: ControllerAddress) -> Result<u64> {
        self.process_handle
            .read_u64(controller + self.offsets.network.controller.m_steamID)
    }

    fn get_spectator_target(&self, pawn: PawnAddress) -> Result<Option<PawnAddress>> {
        let observer_services = self
            .process_handle
            .read_u64_address(pawn + self.offsets.network.pawn.m_pObserverServices)?;

        if observer_services.is_null() {
            return Ok(None);
        }

        let target = self.process_handle.read_u32(
            observer_services + self.offsets.network.observer_service.m_hObserverTarget,
        )? & 0x7fff;

        if target == 0 {
            return Ok(None);
        }

        let v2 = self.process_handle.read_u64(
            self.offsets.interface.player + Address::from(8) * (Address::from(target as u64) >> 9),
        )?;

        if v2 == 0 {
            return Ok(None);
        }

        let entity = self
            .process_handle
            .read_u64_address(v2 + 120 * (target as u64 & 0x1ff))?;

        if entity.is_null() {
            return Ok(None);
        }

        Ok(Some(entity))
    }

    fn get_player(&self, controller: ControllerAddress) -> Result<Option<Player>> {
        let mut player = Player::default();
        let pawn = self.get_pawn(controller)?;

        let team = match self.get_team(pawn)? {
            Some(team) => team,
            None => return Ok(None),
        };

        player.name = self
            .get_name(controller)
            .context("Unable to get player's name")?
            .unwrap_or("Unknown".to_string());
        player.health = self
            .get_health(pawn)
            .context("Unable to get player's health")?;
        player.armor = self
            .get_armor(pawn)
            .context("Unable to get player's armor")?;
        player.money = self
            .get_money(controller)
            .context("Unable to get player's money")?;
        player.team = team;
        player.life_state = self
            .get_life_state(pawn)
            .context("Unable to get player's life state")?
            .unwrap_or_default();
        player.weapon = self
            .get_weapon(pawn)
            .ok()
            .flatten()
            .unwrap_or("Unknown".to_string());
        player.weapons = self
            .get_weapons(pawn)
            .context("Unable to get player's weapons")?;
        player.has_defuser = self
            .get_defuser(pawn)
            .context("Unable to determine if player has defuser")?;
        player.has_helmet = self
            .get_helmet(pawn)
            .context("Unable to determine if player has helmet")?;
        player.color = self
            .get_color(controller)
            .context("Unable to get player's color")?;
        player.position = self
            .get_position(pawn)
            .context("Unable to get player's position")?;
        player.rotation = self
            .get_rotation(pawn)
            .context("Unable to get player's rotation")?;
        player.ping = self
            .get_ping(controller)
            .context("Unable to get player's ping")?;
        player.steam_id = self
            .get_steam_id(controller)
            .context("Unable to get player's Steam ID")?;

        Ok(Some(player))
    }

    pub fn get_players(&self) -> Result<Vec<Player>> {
        let local_controller = self.get_local_controller()?;
        let local_pawn = self.get_pawn(local_controller)?;

        let spectator_target = self.get_spectator_target(local_pawn)?;

        let mut players = vec![];

        for i in 1..=64 {
            let controller = match self
                .get_client_entity(i)
                .context("Unable to get client entity")?
            {
                Some(controller) => controller,
                None => {
                    continue;
                }
            };

            let pawn = match self.get_pawn(controller) {
                Ok(pawn) => pawn,
                Err(_) => continue,
            };

            let mut player = match self
                .get_player(controller)
                .context("Unable to get player")?
            {
                Some(player) => player,
                None => continue,
            };

            player.is_local_player = controller == local_controller;

            if spectator_target.is_some_and(|target| pawn == target) {
                player.active_player = true;
            }

            if spectator_target.is_none() && player.is_local_player {
                player.active_player = true;
            }

            players.push(player);
        }

        Ok(players)
    }

    pub fn get_convar_value_str(&self, convar: &str) -> Result<Option<String>> {
        let convar = *match self.convars.get(convar) {
            Some(address) => address,
            None => return Ok(None),
        };

        let offset = convar + Address::from(64);

        let value = self.process_handle.read_string(offset)?;

        Ok(Some(value))
    }
}
