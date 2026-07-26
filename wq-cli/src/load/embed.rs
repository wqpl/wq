pub struct EmbeddedScript {
    /// Shown in debugger/backtraces
    pub virtual_name: &'static str,
    /// used in angle-bracket loads (e.g., <prelude>)
    pub aliases: &'static [&'static str],
    /// Canonical filename (only for reference)
    // filename: &'static str,
    pub content: &'static str,
}

static EMBEDDED: &[EmbeddedScript] = &[EmbeddedScript {
    virtual_name: "<prelude.wq>",
    aliases: &["prelude"],
    content: include_str!("../../wqstd/prelude.wq"),
}];

pub fn embedded_aliases() -> impl Iterator<Item = &'static str> {
    EMBEDDED
        .iter()
        .flat_map(|script| script.aliases.iter().copied())
}

pub fn lookup_embedded_by_alias(name: &str) -> Option<&'static EmbeddedScript> {
    let n = name.trim().trim_matches(['<', '>']);
    let n = n.strip_suffix(".wq").unwrap_or(n); // .wq optional for embedded alias
    EMBEDDED.iter().find(|e| e.aliases.contains(&n))
}

pub fn lookup_embedded_exact(name: &str) -> Option<&'static EmbeddedScript> {
    EMBEDDED.iter().find(|script| script.virtual_name == name)
}
