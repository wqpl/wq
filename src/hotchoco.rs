// hotchoco: load resolver for wq

use std::{
    cell::RefCell,
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::{
    apps::evaluator::Evaluator,
    vm::GlobalMap,
    wqerror::{WqError, WqErrorType},
};

// Public API =============================================================================

pub fn repl_load_script<T, P>(
    evaluator: &mut T,
    filename: P,
    loading: &RefCell<HashSet<PathBuf>>,
    silent: bool,
    bt: bool,
) -> Result<LoadReport, LoadError>
where
    T: Evaluator,
    P: AsRef<Path>,
{
    let mut loader = Loader::new(evaluator, bt, silent);
    loader.load_script(filename, loading)
}

// Evaluate an inline snippet containing directives (e.g., !p, !load ...),
// using the same streaming logic and reporting as file loads.
pub fn repl_eval_inline<T>(
    evaluator: &mut T,
    content: &str,
    cwd: &Path,
    loading: &RefCell<HashSet<PathBuf>>,
    silent: bool,
    bt: bool,
) -> Result<LoadReport, LoadError>
where
    T: Evaluator,
{
    let mut loader = Loader::new(evaluator, bt, silent);
    let before: GlobalMap = loader.evaluator.env_vars().clone();
    let display_label = "<repl>".to_string();
    let _frame = loader.push_frame(display_label.clone());
    loader.evaluator.dbg_set_source(&display_label, content);
    loader.evaluator.dbg_set_offset(0);
    loader.eval_streaming(content, cwd, &display_label, loading)?;
    // Compute report
    let (new_bindings, overridden) = diff_bindings(loader.evaluator.env_vars(), &before);
    // Prefer the last loaded label when a directive performed a load
    let label = loader.last_loaded_label.clone().unwrap_or(display_label);
    Ok(LoadReport {
        label,
        new_bindings,
        overridden,
        warnings: std::mem::take(&mut loader.warnings),
        result: loader.last_result.take(),
    })
}

// Reporting types =============================================================================

#[derive(Debug, Clone)]
pub struct LoadReport {
    pub label: String,
    pub new_bindings: Vec<String>,
    pub overridden: Vec<String>,
    pub warnings: Vec<String>,
    pub result: Option<crate::value::Value>,
}

#[derive(Debug)]
pub enum LoadErrorKind {
    Cycle(PathBuf),
    Io(PathBuf, io::Error),
    Eval(String, Box<WqError>),
    Directive(String),
}

#[derive(Debug)]
pub struct LoadError {
    pub kind: LoadErrorKind,
    pub stack: Vec<String>, // import stack (A -> B -> C)
}

impl LoadError {
    fn with_stack(kind: LoadErrorKind, stack: &[String]) -> Self {
        Self {
            kind,
            stack: stack.to_vec(),
        }
    }

    pub fn is_runtime(&self) -> bool {
        match &self.kind {
            LoadErrorKind::Eval(_, e) => e.err_type.is_runtime(),
            _ => false,
        }
    }
}

// impl fmt::Display for LoadErrorKind {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         match self {
//             LoadErrorKind::Cycle(path) => {
//                 write!(f, "cannot load {}: cycling", path.display())
//             }
//             LoadErrorKind::Io(path, e) => write!(f, "cannot read {}: {}", path.display(), e),
//             LoadErrorKind::Eval(label, e) => write!(f, "eval error at {label}: {e}"),
//         }
//     }
// }

// impl fmt::Display for LoadError {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         write!(f, "{}", self.kind)
//     }
// }

// impl error::Error for LoadError {}
// impl error::Error for LoadErrorKind {}

// Embedded registry =============================================================================

struct EmbeddedScript {
    /// Shown in debugger/backtraces
    virtual_name: &'static str,
    /// used in angle-bracket loads (e.g., <prelude>)
    aliases: &'static [&'static str],
    /// Canonical filename (only for reference)
    // filename: &'static str,
    content: &'static str,
}

static EMBEDDED: &[EmbeddedScript] = &[
    EmbeddedScript {
        virtual_name: "<prelude.wq>",
        aliases: &["prelude"],
        // filename: "prelude.wq",
        content: include_str!("../wqstd/prelude.wq"),
    },
    // EmbeddedScript {
    //     virtual_name: "<str.wq>",
    //     aliases: &["str"],
    //     // filename: "str.wq",
    //     content: include_str!("../std/str.wq"),
    // },
];

fn lookup_embedded_by_alias(name: &str) -> Option<&'static EmbeddedScript> {
    let n = name.trim().trim_matches(['<', '>']);
    let n = n.strip_suffix(".wq").unwrap_or(n); // .wq optional for embedded alias
    EMBEDDED.iter().find(|e| e.aliases.contains(&n))
}

// Cycle guard =============================================================================
// RAII guard that inserts/removes a key from an external loading set
// This keeps &mut self free while eval calls borrow self
struct CycleGuard<'a> {
    set: &'a RefCell<HashSet<PathBuf>>,
    key: PathBuf,
}

impl<'a> CycleGuard<'a> {
    fn new(set: &'a RefCell<HashSet<PathBuf>>, key: PathBuf) -> Self {
        set.borrow_mut().insert(key.clone());
        Self { set, key }
    }
}

impl<'a> Drop for CycleGuard<'a> {
    fn drop(&mut self) {
        self.set.borrow_mut().remove(&self.key);
    }
}

// Loader =============================================================================

pub struct Loader<'a, T: Evaluator> {
    evaluator: &'a mut T,
    bt: bool,
    silent: bool,
    stack: Rc<RefCell<Vec<String>>>,
    warnings: Vec<String>,
    last_loaded_label: Option<String>,
    last_result: Option<crate::value::Value>,
    embedded_loaded: Rc<RefCell<HashSet<&'static str>>>,
}

impl<'a, T: Evaluator> Loader<'a, T> {
    pub fn new(evaluator: &'a mut T, bt: bool, silent: bool) -> Self {
        Self {
            evaluator,
            bt,
            silent,
            stack: Rc::new(RefCell::new(Vec::new())),
            warnings: Vec::new(),
            last_loaded_label: None,
            last_result: None,
            embedded_loaded: Rc::new(RefCell::new(HashSet::new())),
        }
    }

    pub fn load_script<P: AsRef<Path>>(
        &mut self,
        filename: P,
        loading: &RefCell<HashSet<PathBuf>>,
    ) -> Result<LoadReport, LoadError> {
        let path = filename.as_ref();
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if loading.borrow().contains(&canonical) {
            return Err(LoadError::with_stack(
                LoadErrorKind::Cycle(canonical),
                &self.stack.borrow(),
            ));
        }
        let _guard = CycleGuard::new(loading, canonical.clone());
        let content = fs::read_to_string(path).map_err(|e| {
            LoadError::with_stack(
                LoadErrorKind::Io(path.to_path_buf(), e),
                &self.stack.borrow(),
            )
        })?;
        let display_label = path.display().to_string();
        let _frame = self.push_frame(display_label.clone());
        let before: GlobalMap = self.evaluator.env_vars().clone();
        self.evaluator.dbg_set_source(&display_label, &content);
        self.eval_streaming(
            &content,
            path.parent().unwrap_or_else(|| Path::new("")),
            &display_label,
            loading,
        )?;
        let (new_bindings, overridden) = diff_bindings(self.evaluator.env_vars(), &before);
        self.last_loaded_label = Some(display_label.clone());
        Ok(LoadReport {
            label: display_label,
            new_bindings,
            overridden,
            warnings: std::mem::take(&mut self.warnings),
            result: self.last_result.take(),
        })
    }

    fn eval_streaming(
        &mut self,
        content: &str,
        base_dir: &Path,
        display_label: &str,
        loading: &RefCell<HashSet<PathBuf>>,
    ) -> Result<(), LoadError> {
        // Precompute line start byte offsets
        let mut line_starts = Vec::with_capacity(128);
        line_starts.push(0);
        for (i, b) in content.as_bytes().iter().enumerate() {
            if *b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        if *line_starts.last().unwrap() != content.len() {
            line_starts.push(content.len());
        }
        let mut buffer = String::new();
        let mut buffer_has_code = false;
        let mut consumed_bytes: usize = 0;
        for (i, raw_line) in content.lines().enumerate() {
            let next_consumed = *line_starts.get(i + 1).unwrap_or(&content.len());
            // Shebang on first line
            if i == 0 && raw_line.starts_with("#!") {
                consumed_bytes = next_consumed;
                continue;
            }
            let trimmed_leading = raw_line.trim_start();
            let trimmed_all = raw_line.trim();
            // Meta directives only when the buffer has no code
            if !buffer_has_code && trimmed_leading.starts_with('!') {
                match parse_meta_directive(trimmed_leading) {
                    Some(Directive::PreludeAlias) => {
                        // alias for !load <prelude>
                        consumed_bytes = next_consumed;
                        self.load_embedded_or_file(
                            "prelude",
                            base_dir,
                            display_label,
                            content,
                            consumed_bytes,
                            loading,
                        )?;
                        continue;
                    }
                    Some(Directive::LoadEmbeddedOrFile(name)) => {
                        consumed_bytes = next_consumed;
                        self.load_embedded_or_file(
                            &name,
                            base_dir,
                            display_label,
                            content,
                            consumed_bytes,
                            loading,
                        )?;
                        continue;
                    }
                    Some(Directive::LoadPath(path_str)) => {
                        consumed_bytes = next_consumed;
                        let sub_path = resolve_load_path(base_dir, &path_str);
                        let mut nested = Loader::new(self.evaluator, self.bt, self.silent);
                        // Inherit current import stack snapshot for nested loader
                        nested.stack = Rc::new(RefCell::new(self.stack.borrow().clone()));
                        // Share the embedded registry across this call graph
                        nested.embedded_loaded = self.embedded_loaded.clone();
                        let child = nested.load_script(&sub_path, loading)?;
                        self.warnings.extend(child.warnings);
                        self.last_loaded_label = Some(child.label);
                        if let Some(result) = child.result {
                            self.last_result = Some(result);
                        }
                        // Restore parent file source context
                        self.evaluator.dbg_set_source(display_label, content);
                        self.evaluator.dbg_set_offset(consumed_bytes);
                        continue;
                    }
                    None => {
                        return Err(LoadError::with_stack(
                            LoadErrorKind::Directive(trimmed_leading.to_string()),
                            &self.stack.borrow(),
                        ));
                    }
                }
            }
            // Preserve original line to keep byte positions
            buffer.push_str(raw_line);
            buffer.push('\n');
            if trimmed_all.is_empty() || trimmed_all.starts_with("//") {
                continue;
            }
            buffer_has_code = true;
            self.evaluator.dbg_set_offset(consumed_bytes);
            match self.evaluator.eval_string(&buffer) {
                Ok(result) => {
                    self.last_result = Some(result);
                    buffer.clear();
                    buffer_has_code = false;
                    consumed_bytes = next_consumed;
                }
                Err(err) => {
                    if err.err_type == WqErrorType::Eof {
                        // continue accumulating lines for this chunk
                        continue;
                    } else {
                        return Err(LoadError::with_stack(
                            LoadErrorKind::Eval(display_label.to_string(), Box::new(err)),
                            &self.stack.borrow(),
                        ));
                    }
                }
            }
        }
        // Flush any trailing chunk
        if !buffer.trim().is_empty() {
            self.evaluator.dbg_set_offset(consumed_bytes);
            match self.evaluator.eval_string(&buffer) {
                Ok(result) => {
                    self.last_result = Some(result);
                }
                Err(err) => {
                    return Err(LoadError::with_stack(
                        LoadErrorKind::Eval(display_label.to_string(), Box::new(err)),
                        &self.stack.borrow(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn load_embedded_or_file(
        &mut self,
        name: &str,
        base_dir: &Path,
        parent_label: &str,
        parent_content: &str,
        restore_offset: usize,
        loading: &RefCell<HashSet<PathBuf>>,
    ) -> Result<(), LoadError> {
        if let Some(script) = lookup_embedded_by_alias(name) {
            // idempotent for any embedded script
            let vname = script.virtual_name;
            if self.embedded_loaded.borrow().contains(&vname) {
                return Ok(());
            }
            // Push the embedded script frame on the import stack
            let _frame = self.push_frame(script.virtual_name.to_string());
            // Temporarily switch debug source to the embedded script
            self.evaluator
                .dbg_set_source(script.virtual_name, script.content);
            self.evaluator.dbg_set_offset(0);
            match self.evaluator.eval_string(script.content) {
                Ok(result) => {
                    self.last_result = Some(result);
                }
                Err(err) => {
                    // Create the error while the current frame is present
                    let err = LoadError::with_stack(
                        LoadErrorKind::Eval(script.virtual_name.to_string(), Box::new(err)),
                        &self.stack.borrow(),
                    );
                    return Err(err);
                }
            }
            // Restore parent file source context
            self.evaluator.dbg_set_source(parent_label, parent_content);
            self.evaluator.dbg_set_offset(restore_offset);
            // mark embedded as loaded after success
            self.embedded_loaded.borrow_mut().insert(vname);
            // Remember last loaded label as the embedded script name
            self.last_loaded_label = Some(script.virtual_name.to_string());
            Ok(())
        } else {
            // fall back to a literal file and record a warning.
            self.warnings.push(format!(
                "'{name}' is not found in embedded scripts; attempting to load as a file",
            ));
            let sub_path = resolve_load_path(base_dir, name);
            let mut nested = Loader::new(self.evaluator, self.bt, self.silent);
            nested.stack = Rc::new(RefCell::new(self.stack.borrow().clone()));
            nested.embedded_loaded = self.embedded_loaded.clone();
            let child = nested.load_script(&sub_path, loading)?;
            self.warnings.extend(child.warnings);
            self.last_loaded_label = Some(child.label);
            if let Some(result) = child.result {
                self.last_result = Some(result);
            }
            // Restore parent file source context
            self.evaluator.dbg_set_source(parent_label, parent_content);
            self.evaluator.dbg_set_offset(restore_offset);
            Ok(())
        }
    }
}

// RAII guard for stack frames on Loader.import stack
struct StackFrameGuard {
    stack: Rc<RefCell<Vec<String>>>,
    prev_len: usize,
}

impl Drop for StackFrameGuard {
    fn drop(&mut self) {
        // Truncate back to previous length on scope exit
        let mut st = self.stack.borrow_mut();
        st.truncate(self.prev_len);
    }
}

impl<'a, T: Evaluator> Loader<'a, T> {
    fn push_frame(&self, label: String) -> StackFrameGuard {
        let mut st = self.stack.borrow_mut();
        let prev_len = st.len();
        st.push(label);
        StackFrameGuard {
            stack: self.stack.clone(),
            prev_len,
        }
    }
}

// Utilities ==================================================================

// Diff specialized for HashMap<String, Value> using only names, per request.
fn diff_bindings(after: &GlobalMap, before: &GlobalMap) -> (Vec<String>, Vec<String>) {
    let mut new_bindings = Vec::new();
    let mut overridden = Vec::new();
    for (name, val_after) in after.iter() {
        match before.get(name) {
            None => new_bindings.push(name.clone()),
            Some(val_before) => {
                if val_after != val_before {
                    overridden.push(name.clone());
                }
            }
        }
    }
    new_bindings.sort_unstable();
    overridden.sort_unstable();
    (new_bindings, overridden)
}

// Resolve relative paths against the including file's directory
fn resolve_load_path(base_dir: &Path, arg: &str) -> PathBuf {
    let p = Path::new(arg);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base_dir.join(p)
    }
}

// meta-directive parser
#[derive(Debug)]
enum Directive {
    PreludeAlias,
    LoadEmbeddedOrFile(String),
    LoadPath(String),
}

fn parse_meta_directive(line: &str) -> Option<Directive> {
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
