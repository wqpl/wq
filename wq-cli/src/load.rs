pub mod embed;
pub mod report;

use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use wqpl::script::{ScriptDirective, ScriptItem, ScriptSpan, parse_script_items};
use wqpl::session::Session;
use wqpl::value::Value;
use wqpl::vm::GlobalMap;

use crate::load::embed::lookup_embedded_by_alias;
use crate::load::report::{LoadError, LoadErrorKind, LoadReport};

struct Loader<'a> {
    session: &'a mut Session,
    silent: bool,
    stack: Rc<RefCell<Vec<String>>>,
    warnings: Vec<String>,
    last_loaded_label: Option<String>,
    last_result: Option<Value>,
    embedded_loaded: Rc<RefCell<HashSet<&'static str>>>,
}

impl<'a> Loader<'a> {
    fn new(session: &'a mut Session, silent: bool) -> Self {
        Self {
            session,
            silent,
            stack: Rc::new(RefCell::new(Vec::new())),
            warnings: Vec::new(),
            last_loaded_label: None,
            last_result: None,
            embedded_loaded: Rc::new(RefCell::new(HashSet::new())),
        }
    }

    fn load_script<P: AsRef<Path>>(
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
        let before: GlobalMap = self.session.env_vars();
        self.session.dbg_set_source(&display_label, &content);
        self.eval_streaming(
            &content,
            path.parent().unwrap_or_else(|| Path::new("")),
            &display_label,
            loading,
        )?;
        let after = self.session.env_vars();
        let (new_bindings, overridden) = diff_bindings(&after, &before);
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
        for item in parse_script_items(content) {
            match item {
                ScriptItem::Shebang { .. } => {}
                ScriptItem::Directive(directive) => {
                    self.eval_directive(directive, base_dir, display_label, content, loading)?;
                }
                ScriptItem::Code { span } => {
                    self.eval_code_span(content, display_label, span)?;
                }
            }
        }
        Ok(())
    }

    fn eval_code_span(
        &mut self,
        content: &str,
        display_label: &str,
        span: ScriptSpan,
    ) -> Result<(), LoadError> {
        let chunk = &content[span.as_range()];
        if chunk.trim().is_empty() {
            return Ok(());
        }
        self.session.dbg_set_offset(span.start);
        match self.session.eval_string(chunk) {
            Ok(result) => {
                self.last_result = Some(result);
                Ok(())
            }
            Err(err) => Err(LoadError::with_stack(
                LoadErrorKind::Eval(display_label.to_string(), Box::new(err)),
                &self.stack.borrow(),
            )),
        }
    }

    fn eval_directive(
        &mut self,
        directive: ScriptDirective,
        base_dir: &Path,
        display_label: &str,
        content: &str,
        loading: &RefCell<HashSet<PathBuf>>,
    ) -> Result<(), LoadError> {
        let restore_offset = directive.span().end;
        match directive {
            ScriptDirective::PreludeAlias { .. } => {
                self.load_embedded_or_file(
                    "prelude",
                    base_dir,
                    display_label,
                    content,
                    restore_offset,
                    loading,
                )?;
            }
            ScriptDirective::LoadEmbeddedOrFile { name, .. } => {
                self.load_embedded_or_file(
                    &name,
                    base_dir,
                    display_label,
                    content,
                    restore_offset,
                    loading,
                )?;
            }
            ScriptDirective::LoadPath { path, .. } => {
                let sub_path = resolve_load_path(base_dir, &path);
                let mut nested = Loader::new(self.session, self.silent);
                // Inherit current import stack snapshot for nested loader.
                nested.stack = Rc::new(RefCell::new(self.stack.borrow().clone()));
                // Share the embedded registry across this call graph.
                nested.embedded_loaded = self.embedded_loaded.clone();
                let child = nested.load_script(&sub_path, loading)?;
                self.warnings.extend(child.warnings);
                self.last_loaded_label = Some(child.label);
                if let Some(result) = child.result {
                    self.last_result = Some(result);
                }
                self.session.dbg_set_source(display_label, content);
                self.session.dbg_set_offset(restore_offset);
            }
            ScriptDirective::Unknown { text, .. } => {
                return Err(LoadError::with_stack(
                    LoadErrorKind::Directive(text),
                    &self.stack.borrow(),
                ));
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
            self.session
                .dbg_set_source(script.virtual_name, script.content);
            self.session.dbg_set_offset(0);
            match self.session.eval_string(script.content) {
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
            self.session.dbg_set_source(parent_label, parent_content);
            self.session.dbg_set_offset(restore_offset);
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
            let mut nested = Loader::new(self.session, self.silent);
            nested.stack = Rc::new(RefCell::new(self.stack.borrow().clone()));
            nested.embedded_loaded = self.embedded_loaded.clone();
            let child = nested.load_script(&sub_path, loading)?;
            self.warnings.extend(child.warnings);
            self.last_loaded_label = Some(child.label);
            if let Some(result) = child.result {
                self.last_result = Some(result);
            }
            // Restore parent file source context
            self.session.dbg_set_source(parent_label, parent_content);
            self.session.dbg_set_offset(restore_offset);
            Ok(())
        }
    }

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

pub fn load_script<P>(
    evaluator: &mut Session,
    filename: P,
    loading: &RefCell<HashSet<PathBuf>>,
    silent: bool,
) -> Result<LoadReport, LoadError>
where
    P: AsRef<Path>,
{
    let mut loader = Loader::new(evaluator, silent);
    loader.load_script(filename, loading)
}

// Evaluate an inline snippet containing directives (e.g., !p, !load ...),
// using the same streaming logic and reporting as file loads.
pub fn eval_inline_with_load(
    session: &mut Session,
    content: &str,
    cwd: &Path,
    loading: &RefCell<HashSet<PathBuf>>,
    silent: bool,
) -> Result<LoadReport, LoadError> {
    let mut loader = Loader::new(session, silent);
    let before: GlobalMap = loader.session.env_vars();
    let display_label = "<script>".to_string();
    let _frame = loader.push_frame(display_label.clone());
    loader.session.dbg_set_source(&display_label, content);
    loader.session.dbg_set_offset(0);
    loader.eval_streaming(content, cwd, &display_label, loading)?;
    // Compute report
    let after = loader.session.env_vars();
    let (new_bindings, overridden) = diff_bindings(&after, &before);
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
