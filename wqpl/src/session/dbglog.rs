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

    pub const CST: u16 = 1 << 10;

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

    fn remove(&mut self, bit: u16) {
        self.bits &= !bit;
        // Removing a base flag also removes the paired verbose flag so the
        // verbose flag cannot remain active without anything to make verbose.
        if let Some(verbose) = verbose_bit_for_base(bit) {
            self.bits &= !verbose;
        }
    }

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

    fn apply_flags(&mut self, flags: Self, enabled: bool) {
        for &(_, bit) in &DEBUG_LOG_FLAG_NAMES {
            if flags.contains(bit) {
                if enabled {
                    self.insert(bit);
                } else {
                    self.remove(bit);
                }
            }
        }
    }

    fn parse_part(part: &str) -> Result<Self, String> {
        if let Ok(level) = part.parse::<u8>() {
            return Self::from_alias(level).ok_or_else(|| format!("invalid debug alias '{level}'"));
        }
        if let Some(bit) = bit_for_name(part) {
            let mut flags = Self::empty();
            flags.insert(bit);
            return Ok(flags);
        }
        Err(format!("invalid debug flag '{part}'"))
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

    pub fn apply_spec(&mut self, spec: &str) -> Result<(), String> {
        if spec.is_empty() {
            *self = Self::from_alias(1).expect("debug alias 1 exists");
            return Ok(());
        }

        let mut rewrite = false;
        for raw_part in spec.split(',') {
            let part = raw_part.trim();
            if part.is_empty() {
                continue;
            }

            let (enabled, item) = if let Some(item) = part.strip_prefix('+') {
                (true, item)
            } else if let Some(item) = part.strip_prefix('-') {
                (false, item)
            } else {
                if !rewrite {
                    *self = Self::empty();
                    rewrite = true;
                }
                (true, part)
            };

            let flags = Self::parse_part(item)?;
            self.apply_flags(flags, enabled);
        }
        Ok(())
    }

    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut flags = Self::empty();
        flags.apply_spec(spec)?;
        Ok(flags)
    }
}

pub const DEBUG_LOG_FLAG_NAMES: [(&[&str], u16); 11] = [
    (&["token", "t"], DebugLogFlags::TOKEN),
    (&["cst"], DebugLogFlags::CST),
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

const fn verbose_bit_for_base(bit: u16) -> Option<u16> {
    match bit {
        DebugLogFlags::AST => Some(DebugLogFlags::AST_VERBOSE),
        DebugLogFlags::INST => Some(DebugLogFlags::INST_VERBOSE),
        DebugLogFlags::WQDB => Some(DebugLogFlags::WQDB_VERBOSE),
        DebugLogFlags::CAS => Some(DebugLogFlags::CAS_VERBOSE),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cst_flag_parses_and_displays() {
        let flags = DebugLogFlags::parse("cst").expect("parse cst debug flag");

        assert!(flags.contains(DebugLogFlags::CST));
        assert_eq!(flags.display_names(), vec!["cst"]);
    }

    #[test]
    fn modifier_specs_update_existing_flags() {
        let mut flags = DebugLogFlags::from_names(["inst"]);

        flags
            .apply_spec("+ast,+value")
            .expect("apply additive debug spec");
        assert_eq!(flags.display_names(), vec!["ast", "inst", "value"]);

        flags
            .apply_spec("-inst")
            .expect("apply subtractive debug spec");
        assert_eq!(flags.display_names(), vec!["ast", "value"]);

        flags
            .apply_spec("token")
            .expect("apply overwrite debug spec");
        assert_eq!(flags.display_names(), vec!["token"]);
    }

    #[test]
    fn removing_base_flag_also_removes_verbose_flag() {
        let mut flags = DebugLogFlags::from_names(["ast-v", "inst-v"]);

        flags
            .apply_spec("-ast")
            .expect("apply subtractive debug spec");

        assert_eq!(flags.display_names(), vec!["inst", "inst-v"]);
    }
}
