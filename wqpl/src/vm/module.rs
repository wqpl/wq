use std::borrow::Cow;
use std::sync::Arc;

use crate::compile::Compiler;
use crate::lex::Lexer;
use crate::module::{ModuleRequest, ResolvedModule};
use crate::parse::resolve::Resolver;
use crate::parse::{Parser, fold};
use crate::script::{ScriptItem, parse_script_items};
use crate::value::func::FunctionData;
use crate::value::{Value, WqResult};
use crate::vm::call::CallSpec;
use crate::vm::inst::ImportData;
use crate::vm::{ModuleCacheEntry, Vm};
use crate::wqerror::{WqError, WqErrorType};

impl Vm {
    pub(crate) fn import_module(&mut self, import: &ImportData) -> WqResult<Option<Value>> {
        let resolver = self.module_resolver.clone().ok_or_else(|| {
            WqError::new(WqErrorType::Io)
                .src("module resolver")
                .msg(format!(
                    "cannot import '{}': this host has no module resolver",
                    import.specifier
                ))
        })?;
        let request =
            ModuleRequest::new(Arc::clone(&import.specifier), Arc::clone(&import.importer));
        let resolved = resolver.resolve(&request).map_err(|error| {
            WqError::new(WqErrorType::Io)
                .src("module resolver")
                .msg(format!("cannot import '{}': {error}", import.specifier))
        })?;
        let identity: Arc<str> = Arc::from(resolved.identity());

        match self.module_cache.get(&identity) {
            Some(ModuleCacheEntry::Loaded(value)) => return Ok(Some(value.clone())),
            Some(ModuleCacheEntry::Loading) => {
                let mut chain: Vec<&str> = self.module_loading.iter().map(AsRef::as_ref).collect();
                chain.push(&identity);
                return Err(WqError::new(WqErrorType::Exec)
                    .src("module loader")
                    .msg(format!("module import cycle: {}", chain.join(" -> "))));
            }
            None => {}
        }

        self.module_cache
            .insert(Arc::clone(&identity), ModuleCacheEntry::Loading);
        self.module_loading.push(Arc::clone(&identity));

        let initializer = match self.compile_module(&resolved) {
            Ok(initializer) => initializer,
            Err(error) => {
                self.fail_module(&identity);
                return Err(error);
            }
        };
        let initializer = Value::CompiledFunction(Arc::new(initializer));
        let spec = CallSpec::from_user_callable(
            &initializer,
            0,
            Some(Cow::Owned(format!("<module {}>", resolved.path()))),
        )
        .expect("module initializer is a compiled function");
        if let Err(error) = self.enter_spec(spec) {
            self.fail_module(&identity);
            return Err(error);
        }
        self.execution_frames
            .last_mut()
            .expect("entering a module creates an execution frame")
            .module_identity = Some(identity);
        Ok(None)
    }

    fn compile_module(&mut self, module: &ResolvedModule) -> WqResult<FunctionData> {
        if parse_script_items(module.source())
            .iter()
            .any(|item| matches!(item, ScriptItem::Directive(_)))
        {
            return Err(WqError::new(WqErrorType::Syntax)
                .src("module loader")
                .msg("legacy script directives are not allowed in imported modules")
                .source_ctx(module.source(), module.path()));
        }

        let mut lexer = Lexer::new(module.source());
        lexer.set_source_path(module.path().to_string());
        let tokens = lexer.tokenize()?;
        let builtins = self.builtins.clone();
        let mut parser =
            Parser::new_with_builtins(tokens, module.source().to_string(), builtins.clone());
        parser.set_source_path(module.path().to_string());
        let ast = parser.parse()?;
        if let Some(error) = parser.eof_error() {
            return Err(error.clone());
        }
        let mut resolver = Resolver::with_builtins(builtins.clone());
        let ast = fold::fold(resolver.resolve(ast));

        let mut compiler = Compiler::new_with_builtins(builtins);
        compiler.set_fn_spans(parser.fn_body_spans_all().clone());
        compiler.set_source(module.source().to_string());
        compiler.set_source_path(module.path().to_string());
        compiler.set_import_origin(module.import_origin().to_string());
        compiler.set_stmt_spans(parser.stmt_spans_top().to_vec());
        let mut initializer = compiler.compile_module_initializer(&ast)?;

        if self.debug_artifacts_enabled() {
            let file_id = self.debug_info.new_file(module.path(), module.source());
            let chunk = self.debug_info.new_function_chunk(
                Some(Arc::from(format!("<module {}>", module.path()))),
                file_id,
                initializer.instructions.len(),
            );
            initializer.dbg_chunk = Some(chunk);
        }
        Ok(initializer)
    }

    pub(crate) fn finish_module(&mut self, identity: Arc<str>, value: Value) {
        self.remove_loading_identity(&identity);
        self.module_cache
            .insert(identity, ModuleCacheEntry::Loaded(value));
    }

    pub(crate) fn fail_module(&mut self, identity: &str) {
        self.remove_loading_identity(identity);
        if matches!(
            self.module_cache.get(identity),
            Some(ModuleCacheEntry::Loading)
        ) {
            self.module_cache.remove(identity);
        }
    }

    fn remove_loading_identity(&mut self, identity: &str) {
        if self
            .module_loading
            .last()
            .is_some_and(|candidate| candidate.as_ref() == identity)
        {
            self.module_loading.pop();
        } else if let Some(index) = self
            .module_loading
            .iter()
            .rposition(|candidate| candidate.as_ref() == identity)
        {
            self.module_loading.remove(index);
        }
    }

    pub(crate) fn clear_loading_modules(&mut self) {
        for identity in self.module_loading.drain(..) {
            if matches!(
                self.module_cache.get(&identity),
                Some(ModuleCacheEntry::Loading)
            ) {
                self.module_cache.remove(&identity);
            }
        }
    }
}
