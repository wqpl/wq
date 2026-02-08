use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DebugFlags {
    bits: u8,
}

impl DebugFlags {
    pub const TOKEN: u8 = 1 << 0;
    pub const AST: u8 = 1 << 1;
    pub const INST: u8 = 1 << 2;
    pub const WQDB_1: u8 = 1 << 3;
    pub const WQDB_2: u8 = 1 << 4;

    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self { bits }
    }

    pub const fn bits(self) -> u8 {
        self.bits
    }

    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub const fn contains(self, bit: u8) -> bool {
        (self.bits & bit) != 0
    }

    pub fn insert(&mut self, bit: u8) {
        self.bits |= bit;
    }

    pub fn remove(&mut self, bit: u8) {
        self.bits &= !bit;
    }

    pub fn union(self, other: Self) -> Self {
        Self::from_bits(self.bits | other.bits)
    }

    pub fn from_alias(level: u8) -> Option<Self> {
        match level {
            0 => Some(Self::empty()),
            1 => Some(Self::from_names(["inst"])),
            2 => Some(Self::from_names(["inst", "ast"])),
            3 => Some(Self::from_names(["inst", "ast", "token"])),
            4 => Some(Self::from_names([
                "inst", "ast", "token", "wqdb-1", "wqdb-2",
            ])),
            _ => None,
        }
    }

    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut flags = Self::empty();
        for name in names {
            if let Some(bit) = bit_for_name(name.as_ref()) {
                flags.insert(bit);
            }
        }
        flags
    }

    pub fn display_names(self) -> Vec<&'static str> {
        let mut names = Vec::new();
        for &(name, bit) in &DEBUG_FLAG_NAMES {
            if self.contains(bit) {
                names.push(name);
            }
        }
        names
    }

    pub fn parse(spec: &str) -> Result<Self, String> {
        if spec.is_empty() {
            return Ok(Self::from_alias(1).unwrap());
        }
        if let Ok(level) = spec.parse::<u8>() {
            return Self::from_alias(level).ok_or_else(|| format!("invalid debug alias '{level}'"));
        }
        let mut flags = Self::empty();
        for name in spec
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            if let Some(bit) = bit_for_name(name) {
                flags.insert(bit);
            } else {
                return Err(format!("invalid debug flag '{name}'"));
            }
        }
        Ok(flags)
    }
}

pub const DEBUG_FLAG_NAMES: [(&str, u8); 5] = [
    ("token", DebugFlags::TOKEN),
    ("ast", DebugFlags::AST),
    ("inst", DebugFlags::INST),
    ("wqdb-1", DebugFlags::WQDB_1),
    ("wqdb-2", DebugFlags::WQDB_2),
];

pub fn bit_for_name(name: &str) -> Option<u8> {
    DEBUG_FLAG_NAMES
        .iter()
        .find_map(|(known, bit)| (*known == name).then_some(*bit))
}

pub fn is_known_debug_name(name: &str) -> bool {
    bit_for_name(name).is_some()
}

static DEBUG_FLAGS: AtomicU8 = AtomicU8::new(0);

pub fn set_debug_flags(flags: DebugFlags) {
    DEBUG_FLAGS.store(flags.bits(), Ordering::Relaxed);
}

pub fn get_debug_flags() -> DebugFlags {
    DebugFlags::from_bits(DEBUG_FLAGS.load(Ordering::Relaxed))
}
