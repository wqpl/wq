use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde_json::Value as JsonValue;
use wqpl::module::{ModuleError, ModuleRequest, ModuleResolver, ResolvedModule};
use wqpl::session::Session;

use crate::support::{ResultContext as _, TestResult, test_error};

#[derive(Debug)]
struct WqFence {
    file: PathBuf,
    info: String,
    code: String,
    contract: Option<JsonValue>,
}

#[derive(Clone)]
struct ArticleModuleResolver {
    modules: Arc<HashMap<String, String>>,
}

impl ModuleResolver for ArticleModuleResolver {
    fn resolve(&self, request: &ModuleRequest) -> Result<ResolvedModule, ModuleError> {
        let source = self.modules.get(request.specifier()).ok_or_else(|| {
            ModuleError::new(format!(
                "virtual article module '{}' is not registered",
                request.specifier()
            ))
        })?;
        Ok(ResolvedModule::new(
            request.specifier(),
            request.specifier(),
            source.clone(),
        ))
    }
}

#[test]
fn article_wq_fences_follow_the_portable_example_contract() -> TestResult {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .expect("wq-cli has a workspace parent");
    let mut fences = collect_wq_fences(&workspace.join("d/articles"))?;
    fences.extend(collect_wq_fences(&manifest_dir.join("book"))?);

    let mut failures = Vec::new();
    let mut example_ids = HashSet::new();
    let mut workspaces: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut cell_group_counts: HashMap<(PathBuf, String), usize> = HashMap::new();
    for fence in &fences {
        let Some(contract) = fence.contract.as_ref() else {
            continue;
        };
        if let Some(id) = contract.get("id").and_then(JsonValue::as_str)
            && !example_ids.insert(id.to_string())
        {
            failures.push(format!("duplicate wq-example id '{id}'"));
        }
        if let (Some(workspace), Some(file)) = (
            contract.get("workspace").and_then(JsonValue::as_str),
            contract.get("file").and_then(JsonValue::as_str),
        ) {
            workspaces
                .entry(workspace.to_string())
                .or_default()
                .insert(file.to_string(), fence.code.clone());
        }
        if let Some(cell_group) = contract.get("cellGroup") {
            match cell_group.as_str().filter(|value| !value.trim().is_empty()) {
                Some(cell_group) => {
                    *cell_group_counts
                        .entry((fence.file.clone(), cell_group.to_string()))
                        .or_default() += 1;
                }
                None => failures.push(format!(
                    "{}: wq-example cellGroup must be a non-empty string",
                    fence.file.display()
                )),
            }
        }
    }
    for ((file, cell_group), count) in &cell_group_counts {
        if *count < 2 {
            failures.push(format!(
                "{}: cell group '{cell_group}' has only one cell",
                file.display()
            ));
        }
    }

    let mut cell_group_sessions: HashMap<(PathBuf, String), Session> = HashMap::new();
    for fence in fences {
        if fence.info != "wq" {
            failures.push(format!(
                "{}: use the portable `wq` fence instead of `{}`",
                fence.file.display(),
                fence.info
            ));
            continue;
        }

        let contract = fence.contract.as_ref();
        let role = contract
            .and_then(|value| value.get("role"))
            .and_then(JsonValue::as_str);
        let workspace_name = contract
            .and_then(|value| value.get("workspace"))
            .and_then(JsonValue::as_str);
        let cell_group = contract
            .and_then(|value| value.get("cellGroup"))
            .and_then(JsonValue::as_str);
        let expected_value = contract
            .and_then(|value| value.get("expect"))
            .and_then(|value| value.get("value"))
            .and_then(JsonValue::as_str);
        let expected_error = contract
            .and_then(|value| value.get("expect"))
            .and_then(|value| value.get("error"))
            .and_then(JsonValue::as_str);

        let mut standalone_session = Session::new();
        let session = if let Some(cell_group) = cell_group {
            cell_group_sessions
                .entry((fence.file.clone(), cell_group.to_string()))
                .or_default()
        } else {
            &mut standalone_session
        };
        if let Some(workspace_name) = workspace_name
            && let Some(modules) = workspaces.get(workspace_name)
        {
            session.set_module_resolver(ArticleModuleResolver {
                modules: Arc::new(modules.clone()),
            });
        }

        let result = session.eval_string(&fence.code);
        if role == Some("syntax") {
            if let Err(error) = result
                && matches!(error.err_type.name(), "syntax" | "eof")
            {
                failures.push(format!(
                    "{}: syntax example failed with {error}\n{}",
                    fence.file.display(),
                    fence.code
                ));
            }
            continue;
        }

        match (result, expected_value, expected_error) {
            (Ok(value), Some(expected), None) if value.to_string() != expected => {
                failures.push(format!(
                    "{}: expected value {expected}, got {}\n{}",
                    fence.file.display(),
                    value,
                    fence.code
                ));
            }
            (Ok(_), None, Some(expected)) => {
                failures.push(format!(
                    "{}: expected {expected} error but succeeded\n{}",
                    fence.file.display(),
                    fence.code
                ));
            }
            (Err(error), _, Some(expected)) if error.err_type.name() != expected => {
                failures.push(format!(
                    "{}: expected {expected} error, got {}\n{}",
                    fence.file.display(),
                    error.err_type.name(),
                    fence.code
                ));
            }
            (Err(error), _, None) => {
                failures.push(format!(
                    "{}: fence failed with {error}\n{}",
                    fence.file.display(),
                    fence.code
                ));
            }
            _ => {}
        }
    }

    if !failures.is_empty() {
        return Err(test_error(failures.join("\n\n")));
    }
    Ok(())
}

fn collect_wq_fences(root: &Path) -> TestResult<Vec<WqFence>> {
    let mut files = Vec::new();
    collect_markdown_files(root, &mut files)?;
    let mut fences = Vec::new();
    for file in files {
        let md = fs::read_to_string(&file)
            .with_context(|| format!("read article {}", file.display()))?;
        fences.extend(wq_fences_in_file(&file, &md)?);
    }
    Ok(fences)
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> TestResult {
    for entry in fs::read_dir(dir).with_context(|| format!("read dir {}", dir.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "md") {
            files.push(path);
        }
    }
    Ok(())
}

fn wq_fences_in_file(file: &Path, md: &str) -> TestResult<Vec<WqFence>> {
    let mut fences = Vec::new();
    let mut pending_contract = None;
    let mut current_info: Option<String> = None;
    let mut current_contract = None;
    let mut current_code = String::new();

    for event in Parser::new(md) {
        match event {
            Event::Html(html) | Event::InlineHtml(html) => {
                if let Some(source) = example_directive_json(&html) {
                    pending_contract = Some(serde_json::from_str(source).with_context(|| {
                        format!("invalid wq-example directive in {}", file.display())
                    })?);
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let info = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(info) => info.to_string(),
                    pulldown_cmark::CodeBlockKind::Indented => String::new(),
                };
                current_info = info
                    .split_whitespace()
                    .next()
                    .is_some_and(|lang| lang == "wq")
                    .then_some(info);
                current_contract = pending_contract.take();
                current_code.clear();
            }
            Event::Text(text) if current_info.is_some() => {
                current_code.push_str(&text);
            }
            Event::Text(text) if !text.trim().is_empty() => {
                pending_contract = None;
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(info) = current_info.take() {
                    fences.push(WqFence {
                        file: file.to_path_buf(),
                        info,
                        code: current_code.trim_end().to_string(),
                        contract: current_contract.take(),
                    });
                }
            }
            _ => {}
        }
    }

    Ok(fences)
}

fn example_directive_json(html: &str) -> Option<&str> {
    html.trim()
        .strip_prefix("<!-- wq-example ")
        .and_then(|value| value.strip_suffix("-->"))
        .map(str::trim)
}
