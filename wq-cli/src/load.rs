pub mod embed;
pub mod report;

use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use wqpl::module::{ModuleError, ModuleRequest, ModuleResolver, ResolvedModule};
use wqpl::script::ScriptDirective;
use wqpl::session::{Bindings, DirectiveFailure, ScriptRunError, Session, SourceUnit};
use wqpl::value::Value;

use crate::load::embed::{lookup_embedded_by_alias, lookup_embedded_exact};
use crate::load::report::{LoadError, LoadErrorKind, LoadReport};

struct Loader {
    silent: bool,
    stack: Rc<RefCell<Vec<String>>>,
    warnings: Vec<String>,
    last_loaded_label: Option<String>,
    embedded_loaded: Rc<RefCell<HashSet<&'static str>>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CliModuleResolver;

impl ModuleResolver for CliModuleResolver {
    fn resolve(&self, request: &ModuleRequest) -> Result<ResolvedModule, ModuleError> {
        if let Some(script) = lookup_embedded_exact(request.specifier()) {
            return Ok(ResolvedModule::new(
                script.virtual_name,
                script.virtual_name,
                script.content,
            )
            .with_import_origin(request.importer()));
        }

        let specifier = Path::new(request.specifier());
        let importer = Path::new(request.importer());
        let base = if importer.is_dir() {
            importer
        } else {
            importer.parent().unwrap_or_else(|| Path::new(""))
        };
        let candidate = if specifier.is_absolute() {
            specifier.to_path_buf()
        } else {
            base.join(specifier)
        };
        let canonical = fs::canonicalize(&candidate)
            .map_err(|error| ModuleError::new(format!("{}: {error}", candidate.display())))?;
        let source = fs::read_to_string(&canonical)
            .map_err(|error| ModuleError::new(format!("{}: {error}", canonical.display())))?;
        let identity = canonical.to_string_lossy().into_owned();
        Ok(ResolvedModule::new(identity.clone(), identity, source))
    }
}

pub(crate) fn install_module_resolver(session: &mut Session) {
    session.set_module_resolver(CliModuleResolver);
}

impl Loader {
    fn new(silent: bool) -> Self {
        Self {
            silent,
            stack: Rc::new(RefCell::new(Vec::new())),
            warnings: Vec::new(),
            last_loaded_label: None,
            embedded_loaded: Rc::new(RefCell::new(HashSet::new())),
        }
    }

    fn load_script<P: AsRef<Path>>(
        &mut self,
        session: &mut Session,
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
        let before: Bindings = session.bindings();
        let result = self.eval_streaming(
            session,
            &content,
            path.parent().unwrap_or_else(|| Path::new("")),
            &display_label,
            loading,
        )?;
        let after = session.bindings();
        let (new_bindings, overridden) = diff_bindings(&after, &before);
        self.last_loaded_label = Some(display_label.clone());
        Ok(LoadReport {
            label: display_label,
            new_bindings,
            overridden,
            warnings: std::mem::take(&mut self.warnings),
            result,
        })
    }

    fn eval_streaming(
        &mut self,
        session: &mut Session,
        content: &str,
        base_dir: &Path,
        display_label: &str,
        loading: &RefCell<HashSet<PathBuf>>,
    ) -> Result<Option<Value>, LoadError> {
        let import_origin = base_dir.to_string_lossy();
        let source = SourceUnit::named(display_label, content).with_import_origin(&import_origin);
        let result = session.eval_script_with_postmortem(source, |session, directive| {
            self.eval_directive(session, directive, base_dir, loading)
                .map_err(|error| {
                    DirectiveFailure::classify(error, |error| {
                        error
                            .evaluation_failure()
                            .and_then(|failure| failure.postmortem_token())
                    })
                })
        });
        let result = match result {
            Ok(result) => result,
            Err(ScriptRunError::Evaluation(err)) => {
                return Err(LoadError::with_stack(
                    LoadErrorKind::Eval(display_label.to_string(), Box::new(err)),
                    &self.stack.borrow(),
                ));
            }
            Err(ScriptRunError::Directive(error)) => return Err(error),
        };
        Ok(result)
    }

    fn eval_directive(
        &mut self,
        session: &mut Session,
        directive: ScriptDirective,
        base_dir: &Path,
        loading: &RefCell<HashSet<PathBuf>>,
    ) -> Result<Option<Value>, LoadError> {
        match directive {
            ScriptDirective::PreludeAlias { .. } => {
                self.load_embedded_or_file(session, "prelude", base_dir, loading)
            }
            ScriptDirective::LoadEmbeddedOrFile { name, .. } => {
                self.load_embedded_or_file(session, &name, base_dir, loading)
            }
            ScriptDirective::LoadPath { path, .. } => {
                let sub_path = resolve_load_path(base_dir, &path);
                let mut nested = Loader::new(self.silent);
                // Inherit current import stack snapshot for nested loader.
                nested.stack = Rc::new(RefCell::new(self.stack.borrow().clone()));
                // Share the embedded registry across this call graph.
                nested.embedded_loaded = self.embedded_loaded.clone();
                let child = nested.load_script(session, &sub_path, loading)?;
                self.warnings.extend(child.warnings);
                self.last_loaded_label = Some(child.label);
                Ok(child.result)
            }
            ScriptDirective::Unknown { text, .. } => Err(LoadError::with_stack(
                LoadErrorKind::Directive(text),
                &self.stack.borrow(),
            )),
        }
    }

    fn load_embedded_or_file(
        &mut self,
        session: &mut Session,
        name: &str,
        base_dir: &Path,
        loading: &RefCell<HashSet<PathBuf>>,
    ) -> Result<Option<Value>, LoadError> {
        if let Some(script) = lookup_embedded_by_alias(name) {
            // idempotent for any embedded script
            let vname = script.virtual_name;
            if self.embedded_loaded.borrow().contains(&vname) {
                return Ok(None);
            }
            // Push the embedded script frame on the import stack
            let _frame = self.push_frame(script.virtual_name.to_string());
            let result = self.eval_streaming(
                session,
                script.content,
                base_dir,
                script.virtual_name,
                loading,
            )?;
            // mark embedded as loaded after success
            self.embedded_loaded.borrow_mut().insert(vname);
            // Remember last loaded label as the embedded script name
            self.last_loaded_label = Some(script.virtual_name.to_string());
            Ok(result)
        } else {
            // fall back to a literal file and record a warning.
            self.warnings.push(format!(
                "'{name}' is not found in embedded scripts; attempting to load as a file",
            ));
            let sub_path = resolve_load_path(base_dir, name);
            let mut nested = Loader::new(self.silent);
            nested.stack = Rc::new(RefCell::new(self.stack.borrow().clone()));
            nested.embedded_loaded = self.embedded_loaded.clone();
            let child = nested.load_script(session, &sub_path, loading)?;
            self.warnings.extend(child.warnings);
            self.last_loaded_label = Some(child.label);
            Ok(child.result)
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
    let mut loader = Loader::new(silent);
    loader.load_script(evaluator, filename, loading)
}

// Evaluate an inline snippet containing directives (e.g., \p, \load ...),
// using the same streaming logic and reporting as file loads.
pub fn eval_inline_with_load(
    session: &mut Session,
    content: &str,
    cwd: &Path,
    loading: &RefCell<HashSet<PathBuf>>,
    silent: bool,
) -> Result<LoadReport, LoadError> {
    let mut loader = Loader::new(silent);
    let before: Bindings = session.bindings();
    let display_label = "<script>".to_string();
    let _frame = loader.push_frame(display_label.clone());
    let result = loader.eval_streaming(session, content, cwd, &display_label, loading)?;
    // Compute report
    let after = session.bindings();
    let (new_bindings, overridden) = diff_bindings(&after, &before);
    // Prefer the last loaded label when a directive performed a load
    let label = loader.last_loaded_label.clone().unwrap_or(display_label);
    Ok(LoadReport {
        label,
        new_bindings,
        overridden,
        warnings: std::mem::take(&mut loader.warnings),
        result,
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
fn diff_bindings(after: &Bindings, before: &Bindings) -> (Vec<String>, Vec<String>) {
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
