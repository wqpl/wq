use std::fmt;
use std::sync::Arc;

/// One lexical module request made by an `@i` expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleRequest {
    specifier: Arc<str>,
    importer: Arc<str>,
}

impl ModuleRequest {
    pub(crate) fn new(specifier: Arc<str>, importer: Arc<str>) -> Self {
        Self {
            specifier,
            importer,
        }
    }

    /// The literal specifier written after `@i`.
    pub fn specifier(&self) -> &str {
        &self.specifier
    }

    /// The lexical source origin containing the `@i` expression.
    pub fn importer(&self) -> &str {
        &self.importer
    }
}

/// Source returned by a host module resolver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedModule {
    identity: Arc<str>,
    path: Arc<str>,
    source: Arc<str>,
    import_origin: Arc<str>,
}

impl ResolvedModule {
    /// Create a module whose display path is also its nested-import origin.
    pub fn new(
        identity: impl Into<Arc<str>>,
        path: impl Into<Arc<str>>,
        source: impl Into<Arc<str>>,
    ) -> Self {
        let path = path.into();
        Self {
            identity: identity.into(),
            import_origin: Arc::clone(&path),
            path,
            source: source.into(),
        }
    }

    /// Override the lexical origin used to resolve imports inside this module.
    pub fn with_import_origin(mut self, origin: impl Into<Arc<str>>) -> Self {
        self.import_origin = origin.into();
        self
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn import_origin(&self) -> &str {
        &self.import_origin
    }
}

/// A host failure while resolving or reading a module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleError {
    message: String,
}

impl ModuleError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ModuleError {}

/// Host-provided resolution for `@i` module specifiers.
pub trait ModuleResolver: Send + Sync {
    fn resolve(&self, request: &ModuleRequest) -> Result<ResolvedModule, ModuleError>;
}
