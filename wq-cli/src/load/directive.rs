#[derive(Debug)]
pub enum Directive {
    PreludeAlias,
    LoadEmbeddedOrFile(String),
    LoadPath(String),
}

pub fn parse_meta_directive(line: &str) -> Option<Directive> {
    let s = line.trim();
    if !s.starts_with('!') {
        return None;
    }
    if s == "!p" {
        return Some(Directive::PreludeAlias);
    }
    if let Some(rest) = ["!load", "!l"].iter().find_map(|p| s.strip_prefix(p)) {
        let arg = rest.trim();
        if arg.starts_with('<') && arg.ends_with('>') && arg.len() >= 2 {
            let inner = &arg[1..arg.len() - 1];
            return Some(Directive::LoadEmbeddedOrFile(inner.to_string()));
        }
        if !arg.is_empty() {
            return Some(Directive::LoadPath(arg.to_string()));
        }
    }
    None
}
