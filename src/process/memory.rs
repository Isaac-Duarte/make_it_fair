use std::fmt::Display;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Address(pub(crate) u64);

impl Address {
    pub const NULL: Address = Address(0);

    #[inline]
    pub const fn null() -> Self {
        Address::NULL
    }

    #[inline]
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub fn non_null(self) -> Option<Address> {
        if self.is_null() {
            None
        } else {
            Some(self)
        }
    }

    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

impl From<u64> for Address {
    fn from(value: u64) -> Self {
        Address(value)
    }
}

impl From<Address> for u64 {
    fn from(value: Address) -> Self {
        value.0
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for Address {
    fn default() -> Self {
        Address::null()
    }
}

pub fn check_elf_header(data: &[u8]) -> bool {
    data.len() >= 4 && data[0..4] == [0x7f, b'E', b'L', b'F']
}

pub fn read_u64_vec(data: &[u8], address: u64) -> u64 {
    let adr = address as usize;
    let buffer = [
        data[adr],
        data[adr + 1],
        data[adr + 2],
        data[adr + 3],
        data[adr + 4],
        data[adr + 5],
        data[adr + 6],
        data[adr + 7],
    ];

    u64::from_ne_bytes(buffer)
}

pub fn read_string_vec(data: &[u8], address: u64) -> String {
    let mut string = String::new();
    let mut i = address;
    loop {
        let c = data[i as usize];
        if c == 0 {
            break;
        }
        string.push(c as char);
        i += 1;
    }
    string
}

#[allow(unused)]
pub fn read_u32_vec(data: &[u8], address: u64) -> Address {
    let adr = address as usize;
    let buffer = [data[adr], data[adr + 1], data[adr + 2], data[adr + 3]];
    let val: u64 = u32::from_ne_bytes(buffer) as u64;

    val.into()
}
