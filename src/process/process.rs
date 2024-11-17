use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader},
    os::unix::fs::FileExt,
};

use crate::constant;

use super::{
    memory::{self, Address},
    pid::Pid,
};
use anyhow::{bail, Context, Ok, Result};
use log::{debug, warn};

pub struct ProcessHandle {
    pub pid: Pid,
    pub memory: File,
}

impl ProcessHandle {
    fn new(pid: Pid, memory: File) -> Self {
        Self { pid, memory }
    }

    /// Obtains a `ProcessHandle` given the PID. Note this will only open the memory in read mode.
    pub async fn from_pid(pid: Pid) -> Result<Self> {
        if !pid.validate() {
            bail!("Process is no longer alive")
        }

        let memory = OpenOptions::new()
            .read(true)
            .write(false)
            .open(format!("/proc/{}/mem", pid.0))
            .context("Unable to access memory (Read Only)")?;

        Ok(Self::new(pid, memory))
    }

    /// Retrieves the base address of a specified module for a given process.
    ///
    /// This function reads the memory mapping information of the target process from the `/proc/[pid]/maps`
    /// file to locate the base address of the specified module.
    pub fn get_module_base_address(&self, module_name: &str) -> Result<Address> {
        let maps = File::open(format!("/proc/{}/maps", self.pid))
            .context("Unable to read memory map file")?;

        let reader = BufReader::new(maps).lines();

        for line in reader {
            let line = line?;

            if !line.contains(module_name) {
                continue;
            }

            let (address, _) = line.split_once('-').unwrap();
            let address = u64::from_str_radix(address, 16).context("Unable to parse address")?;

            return Ok(Address::from(address));
        }

        bail!("Unable to find base address")
    }

    /// Retrieves the offset for a specific interface in a module by locating its entry in the
    /// `CreateInterface` export function and resolving the corresponding virtual function table (vfunc) address.
    pub fn get_interface_offset(
        &self,
        base_address: u64,
        interface_name: &str,
    ) -> Result<Option<u64>> {
        // Resolve the CreateInterface export function
        let create_interface = self
            .get_module_export(base_address, "CreateInterface")?
            .context("Unable to resolve CreateInterface export")?;

        // Compute the address of the interface entry list
        let export_address = self.get_relative_address(create_interface, 0x01, 0x05)? + 0x10;

        // Read the first interface entry
        let mut interface_entry = self
            .read_u64(export_address + 0x07 + self.read_u32(export_address + 0x03)? as u64)
            .context("Failed to read the initial interface entry")?;

        debug!(
            "Resolved initial interface entry at address: {:#x}",
            interface_entry
        );

        // Iterate through the linked list of interface entries
        loop {
            // Get the address of the entry's name
            let entry_name_address = self
                .read_u64(interface_entry + 8)
                .context("Failed to read entry name address")?;

            // Read the entry name as a string
            let entry_name = self
                .read_string(entry_name_address)
                .context("Failed to read entry name string")?;

            debug!("Checking interface entry: {}", entry_name);

            // Check if the name starts with the target interface name
            if entry_name.starts_with(interface_name) {
                let vfunc_address = self
                    .read_u64(interface_entry)
                    .context("Failed to read vfunc address")?;

                let interface_offset =
                    self.read_u32(vfunc_address + 0x03)
                        .context("Failed to read interface offset")? as u64
                        + vfunc_address
                        + 0x07;

                debug!(
                    "Found interface '{}' at offset: {:#x}",
                    interface_name, interface_offset
                );
                return Ok(Some(interface_offset));
            }

            // Move to the next entry in the linked list
            interface_entry = self
                .read_u64(interface_entry + 0x10)
                .context("Failed to read next interface entry")?;

            // Check for end of the list
            if interface_entry == 0 {
                break;
            }
        }

        Ok(None)
    }

    /// Retrieves the address of a specified export symbol from a module's dynamic symbol table.
    pub fn get_module_export(&self, base_address: u64, export_name: &str) -> Result<Option<u64>> {
        // Dump the module and validate its ELF header
        let module = self
            .dump_module(base_address)
            .context("Failed to dump module from memory")?;

        if !memory::check_elf_header(&module) {
            bail!("Invalid ELF Header");
        }

        const SYMBOL_TABLE_ENTRY_SIZE: u64 = 0x18;
        const ADDRESS_SIZE: u64 = 0x08;

        // Resolve string table and symbol table addresses
        let string_table = self
            .get_address_from_dynamic_section(base_address, 0x05)?
            .context("Failed to resolve string table address")?;
        let mut symbol_table = self
            .get_address_from_dynamic_section(base_address, 0x06)?
            .context("Failed to resolve symbol table address")?
            + SYMBOL_TABLE_ENTRY_SIZE;

        debug!(
            "String table address: {:#x}, Symbol table address: {:#x}",
            string_table, symbol_table
        );

        // Iterate over the symbol table entries
        while self.read_u32(symbol_table)? != 0 {
            // Read symbol name offset and resolve the symbol name
            let st_name_offset = self
                .read_u32(symbol_table)
                .context("Failed to read symbol name offset")?;
            let symbol_name = self
                .read_string(string_table + st_name_offset as u64)
                .context("Failed to read symbol name")?;

            // debug!("Checking symbol: {}", symbol_name);

            // Compare symbol name with the target export name
            if symbol_name == export_name {
                let address_vec = self
                    .read_bytes(symbol_table + ADDRESS_SIZE, ADDRESS_SIZE)
                    .context("Failed to read symbol address bytes")?;
                let symbol_address = memory::read_u64_vec(&address_vec, 0) + base_address;

                debug!(
                    "Found export '{}' at address: {:#x}",
                    export_name, symbol_address
                );
                return Ok(Some(symbol_address));
            }

            // Move to the next symbol table entry
            symbol_table += SYMBOL_TABLE_ENTRY_SIZE;
        }

        warn!("Export '{}' not found in the module", export_name);
        Ok(None)
    }

    /// Dumps the content of a module from memory into a `Vec<u8>`.
    pub fn dump_module(&self, address: u64) -> Result<Vec<u8>> {
        let module_size = self
            .module_size(address)
            .context("Failed to determine the module size")?;

        self.read_bytes(address, module_size)
            .context("Failed to read module bytes from memory")
    }

    /// Determines the size of a module by calculating the total size of its section headers in memory.
    pub fn module_size(&self, base_address: u64) -> Result<u64> {
        // Read the section header offset from the ELF file header
        let section_header_offset = self
            .read_u64(base_address + constant::ELF_SECTION_HEADER_OFFSET)
            .context("Failed to read section header offset")?;

        // Read the size of each section header entry
        let section_header_entry_size =
            self.read_u16(base_address + constant::ELF_SECTION_HEADER_ENTRY_SIZE)
                .context("Failed to read section header entry size")? as u64;

        // Read the number of section header entries
        let section_header_num_entries =
            self.read_u16(base_address + constant::ELF_SECTION_HEADER_NUM_ENTRIES)
                .context("Failed to read number of section header entries")? as u64;

        // Calculate the total size of the module
        let module_size =
            section_header_offset + section_header_entry_size * section_header_num_entries;

        log::debug!(
            "Module size calculation: offset={:#x}, entry_size={}, num_entries={}, total_size={}",
            section_header_offset,
            section_header_entry_size,
            section_header_num_entries,
            module_size
        );

        Ok(module_size)
    }

    /// Calculates the absolute address from a relative address in an instruction.
    pub fn get_relative_address(
        &self,
        instruction: u64,
        offset: u64,
        instruction_size: u64,
    ) -> Result<u64> {
        // Read the 32-bit signed relative offset from the instruction
        let rip_address = self
            .read_i32(instruction + offset)
            .context("Failed to read relative address from instruction")?;

        // Calculate the resolved absolute address
        let resolved_address = instruction
            .wrapping_add(instruction_size)
            .wrapping_add(rip_address as u64);

        log::debug!(
        "Instruction: {:#x}, Offset: {}, Instruction Size: {}, RIP Address: {}, Resolved Address: {:#x}",
        instruction,
        offset,
        instruction_size,
        rip_address,
        resolved_address
        );

        Ok(resolved_address)
    }

    /// Retrieves the address associated with a specific tag in the dynamic section of an ELF file.
    pub fn get_address_from_dynamic_section(
        &self,
        base_address: u64,
        tag: u64,
    ) -> Result<Option<u64>> {
        // Step 1: Locate the dynamic section offset using the PHT
        let dynamic_section_offset = self
            .get_segment_from_pht(base_address, constant::ELF_DYNAMIC_SECTION_PHT_TYPE)
            .context("Failed to locate the dynamic section in the PHT")?;

        // Step 2: Set constants
        let register_size = 8; // Assumes a 64-bit ELF file
        let mut address = self
            .read_u64(dynamic_section_offset + 2 * register_size)
            .context("Failed to read dynamic section base address")?
            + base_address;

        debug!(
            "Dynamic section located at offset {:#x}, starting address {:#x}",
            dynamic_section_offset, address
        );

        // Step 3: Iterate through the dynamic section entries
        loop {
            let tag_address = address;

            // Read the tag value
            let tag_value = self
                .read_u64(tag_address)
                .context("Failed to read tag value from dynamic section")?;

            // Check if we reached the end of the dynamic section
            if tag_value == 0 {
                debug!(
                    "End of dynamic section reached at address {:#x}",
                    tag_address
                );
                break;
            }

            // Check if the tag matches the requested value
            if tag_value == tag {
                let value_address = self
                    .read_u64(tag_address + register_size)
                    .context("Failed to read value associated with tag")?;
                debug!("Found tag {:#x} with address {:#x}", tag, value_address);

                return Ok(Some(value_address));
            }

            // Move to the next entry (each entry is 2 * register_size bytes)
            address += register_size * 2;
        }

        debug!(
            "Tag {:#x} not found in the dynamic section starting at {:#x}",
            tag, dynamic_section_offset
        );

        Ok(None)
    }

    /// Retrieves the address of a specific program header segment in the Program Header Table (PHT)
    /// of an ELF (Executable and Linkable Format) file based on its tag.
    pub fn get_segment_from_pht(&self, base_address: u64, tag: u64) -> Result<u64> {
        let pht_offset = base_address + constant::ELF_PROGRAM_HEADER_OFFSET;

        let first_entry = self.read_u64(pht_offset)? + base_address;
        let entry_size =
            self.read_u16(base_address + constant::ELF_PROGRAM_HEADER_ENTRY_SIZE)? as u64;

        let num_entries = self.read_u16(base_address + constant::ELF_PROGRAM_HEADER_NUM_ENTRIES)?;

        debug!(
            "Program Header Table: offset={:#x}, entry_size={}, num_entries={}",
            pht_offset, entry_size, num_entries
        );

        (0..num_entries)
            .map(|i| first_entry + i as u64 * entry_size)
            .find(|&entry| self.read_u32(entry).ok().map(|x| x as u64) == Some(tag))
            .context("Tag not found in Program Header Table")
    }

    /// Scans a module's memory for a specific byte pattern using a mask.
    pub fn scan_pattern(
        &self,
        pattern: &[u8],
        mask: &[u8],
        base_address: u64,
    ) -> Result<Option<u64>> {
        // Ensure pattern and mask lengths match
        if pattern.len() != mask.len() {
            bail!(
                "Pattern is {} bytes, mask is {} bytes long. Lengths must match.",
                pattern.len(),
                mask.len()
            );
        }

        let module = self
            .dump_module(base_address)
            .context("Failed to dump module during pattern scan")?;

        if module.len() < pattern.len() {
            return Ok(None);
        }

        let pattern_length = pattern.len();
        let stop_index = module.len() - pattern_length;

        // Scan through the module for the pattern
        for i in 0..stop_index {
            let mut matched = true;

            for j in 0..pattern_length {
                if mask[j] == b'x' && module[i + j] != pattern[j] {
                    matched = false;
                    break;
                }
            }

            // If the pattern is fully matched, calculate and return the absolute address
            if matched {
                let match_address = base_address + i as u64;
                debug!(
                    "Pattern matched at offset {:#x} (absolute address: {:#x})",
                    i, match_address
                );
                return Ok(Some(match_address));
            }
        }

        debug!(
            "Pattern not found in module starting at base address {:#x}",
            base_address
        );

        Ok(None)
    }

    /// Below are read util funcs
    #[allow(unused)]
    pub fn read_i8(&self, address: u64) -> i8 {
        let mut buffer = [0; 1];
        self.memory.read_at(&mut buffer, address).unwrap_or(0);
        i8::from_ne_bytes(buffer)
    }

    #[allow(unused)]
    pub fn read_u8(&self, address: impl Into<u64>) -> Result<u8> {
        let mut buffer = [0; 1];
        self.memory.read_at(&mut buffer, address.into())?;
        Ok(u8::from_ne_bytes(buffer))
    }

    #[allow(unused)]
    pub fn read_u8_address(&self, address: impl Into<u64>) -> Result<Address> {
        let mut buffer = [0; 1];
        self.memory.read_at(&mut buffer, address.into())?;
        let val: u64 = u8::from_ne_bytes(buffer).into();

        Ok(val.into())
    }

    #[allow(unused)]
    pub fn read_i16(&self, address: u64) -> i16 {
        let mut buffer = [0; 2];
        self.memory.read_at(&mut buffer, address).unwrap_or(0);
        i16::from_ne_bytes(buffer)
    }

    #[allow(unused)]
    pub fn read_u16(&self, address: u64) -> Result<u16> {
        let mut buffer = [0; 2];
        self.memory.read_at(&mut buffer, address)?;
        Ok(u16::from_ne_bytes(buffer))
    }

    #[allow(unused)]
    pub fn read_i32(&self, address: impl Into<u64>) -> Result<i32> {
        let mut buffer = [0; 4];
        self.memory.read_at(&mut buffer, address.into())?;
        Ok(i32::from_ne_bytes(buffer))
    }

    #[allow(unused)]
    pub fn read_u32(&self, address: impl Into<u64>) -> Result<u32> {
        let mut buffer = [0; 4];
        self.memory.read_at(&mut buffer, address.into())?;
        Ok(u32::from_ne_bytes(buffer))
    }

    #[allow(unused)]
    pub fn read_i64(&self, address: u64) -> i64 {
        let mut buffer = [0; 8];
        self.memory.read_at(&mut buffer, address).unwrap_or(0);
        i64::from_ne_bytes(buffer)
    }

    #[allow(unused)]
    pub fn read_u64(&self, address: impl Into<u64>) -> Result<u64> {
        let mut buffer = [0; 8];
        self.memory.read_at(&mut buffer, address.into())?;
        Ok(u64::from_ne_bytes(buffer))
    }

    #[allow(unused)]
    pub fn read_u64_address(&self, address: impl Into<u64>) -> Result<Address> {
        let mut buffer = [0; 8];
        self.memory.read_at(&mut buffer, address.into())?;
        Ok(u64::from_ne_bytes(buffer).into())
    }

    #[allow(unused)]
    pub fn read_f32(&self, address: impl Into<u64>) -> Result<f32> {
        let mut buffer = [0; 4];
        self.memory.read_at(&mut buffer, address.into())?;

        Ok(f32::from_ne_bytes(buffer))
    }

    #[allow(unused)]
    pub fn read_f64(&self, address: u64) -> f64 {
        let mut buffer = [0; 8];
        self.memory.read_at(&mut buffer, address).unwrap_or(0);
        f64::from_ne_bytes(buffer)
    }

    #[allow(unused)]
    pub fn read_string(&self, address: impl Into<u64>) -> Result<String> {
        let mut string = String::new();
        let mut i = address.into();

        loop {
            let c = self.read_u8(i).unwrap_or(0);

            if c == 0 {
                break;
            }

            string.push(c as char);
            i += 1;
        }

        Ok(string)
    }

    #[allow(unused)]
    pub fn read_bytes(&self, address: u64, count: u64) -> Result<Vec<u8>> {
        let mut buffer = vec![0u8; count as usize];
        self.memory.read_at(&mut buffer, address)?;

        Ok(buffer)
    }
}

#[cfg(test)]
mod test {
    use super::Pid;
    use crate::{constant, process::process::ProcessHandle};
    use anyhow::Result;

    /// Requires CS2 to be open
    #[tokio::test]
    async fn test_cs2_proc() -> Result<()> {
        let pid = Pid::from_process_name(constant::PROCESS_NAME).await?;
        let result = ProcessHandle::from_pid(pid).await;

        assert!(result.is_ok());

        Ok(())
    }
}
