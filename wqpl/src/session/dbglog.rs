use std::sync::atomic::{AtomicU16, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DebugLogFlags {
    bits: u16,
}

impl DebugLogFlags {
    pub const TOKEN: u16 = 1 << 0;

    pub const AST: u16 = 1 << 1;
    pub const AST_VERBOSE: u16 = 1 << 2;

    pub const INST: u16 = 1 << 3;
    pub const INST_VERBOSE: u16 = 1 << 4;

    pub const WQDB: u16 = 1 << 5;
    pub const WQDB_VERBOSE: u16 = 1 << 6;

    pub const VALUE: u16 = 1 << 7;

    pub const CAS: u16 = 1 << 8;
    pub const CAS_VERBOSE: u16 = 1 << 9;

    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub(crate) const fn from_bits(bits: u16) -> Self {
        Self { bits }
    }

    pub(crate) const fn bits(self) -> u16 {
        self.bits
    }

    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub(crate) const fn contains(self, bit: u16) -> bool {
        (self.bits & bit) != 0
    }

    pub(crate) fn insert(&mut self, bit: u16) {
        self.bits |= bit;
        // When a verbose flag is set, also turn on the corresponding base flag.
        if let Some(base) = base_bit_for_verbose(bit) {
            self.bits |= base;
        }
    }

    // pub(crate) fn remove(&mut self, bit: u8) {
    //     self.bits &= !bit;
    // }

    // pub(crate) fn union(self, other: Self) -> Self {
    //     Self::from_bits(self.bits | other.bits)
    // }

    #[rustfmt::skip]
    pub fn from_alias(level: u8) -> Option<Self> {
        match level {
            // 0=off 1={inst} 2={inst},{ast} 3={inst},{ast},{value} 4={inst},{ast},{value},{inst_v},{ast_v}
            0 => Some(Self::empty()),
            1 => Some(Self::from_names(["inst"])),
            2 => Some(Self::from_names(["inst", "ast"])),
            3 => Some(Self::from_names(["inst", "ast", "value"])),
            4 => Some(Self::from_names(["inst", "ast", "value", "inst-v", "ast-v"])),
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
        for &(knowns, bit) in &DEBUG_LOG_FLAG_NAMES {
            if self.contains(bit) {
                names.push(knowns[0]);
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

pub const DEBUG_LOG_FLAG_NAMES: [(&[&str], u16); 10] = [
    (&["token", "t"], DebugLogFlags::TOKEN),
    (&["ast", "a"], DebugLogFlags::AST),
    (&["ast-v", "av"], DebugLogFlags::AST_VERBOSE),
    (&["inst", "i"], DebugLogFlags::INST),
    (&["inst-v", "iv"], DebugLogFlags::INST_VERBOSE),
    (&["wqdb", "w"], DebugLogFlags::WQDB),
    (&["wqdb-v", "wv"], DebugLogFlags::WQDB_VERBOSE),
    (&["value", "v"], DebugLogFlags::VALUE),
    (&["cas", "c"], DebugLogFlags::CAS),
    (&["cas-v", "cv"], DebugLogFlags::CAS_VERBOSE),
];

pub(crate) fn bit_for_name(name: &str) -> Option<u16> {
    DEBUG_LOG_FLAG_NAMES
        .iter()
        .find_map(|(knowns, bit)| knowns.iter().find(|&&k| k == name).map(|_| *bit))
}

const fn base_bit_for_verbose(bit: u16) -> Option<u16> {
    match bit {
        DebugLogFlags::AST_VERBOSE => Some(DebugLogFlags::AST),
        DebugLogFlags::INST_VERBOSE => Some(DebugLogFlags::INST),
        DebugLogFlags::WQDB_VERBOSE => Some(DebugLogFlags::WQDB),
        DebugLogFlags::CAS_VERBOSE => Some(DebugLogFlags::CAS),
        _ => None,
    }
}

// pub(crate) fn is_known_debug_name(name: &str) -> bool {
//     bit_for_name(name).is_some()
// }

static DEBUG_LOG_FLAGS: AtomicU16 = AtomicU16::new(0);

pub fn set_debug_log_flags(flags: DebugLogFlags) {
    DEBUG_LOG_FLAGS.store(flags.bits(), Ordering::Relaxed);
}

pub fn get_debug_log_flags() -> DebugLogFlags {
    DebugLogFlags::from_bits(DEBUG_LOG_FLAGS.load(Ordering::Relaxed))
}
