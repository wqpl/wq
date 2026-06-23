use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use wqpl::session::Session;

#[derive(Debug)]
struct WqFence {
    file: PathBuf,
    info: String,
    code: String,
}

#[test]
fn article_wq_fences_are_executable_unless_marked() -> Result<()> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("wq-cli has a workspace parent");
    let mut failures = Vec::new();
    for fence in collect_wq_fences(&workspace.join("d/articles"))? {
        let flags: Vec<&str> = fence.info.split_whitespace().collect();
        if flags.contains(&"no-run") {
            continue;
        }
        let mut session = Session::new();
        let result = session.eval_string(&fence.code);
        if flags.contains(&"error") {
            if result.is_ok() {
                failures.push(format!(
                    "{}: fence marked error but succeeded\n{}",
                    fence.file.display(),
                    fence.code
                ));
            }
        } else if let Err(err) = result {
            failures.push(format!(
                "{}: fence failed with {err}\n{}",
                fence.file.display(),
                fence.code
            ));
        }
    }

    if !failures.is_empty() {
        bail!("{}", failures.join("\n\n"));
    }
    Ok(())
}

fn collect_wq_fences(root: &Path) -> Result<Vec<WqFence>> {
    let mut files = Vec::new();
    collect_markdown_files(root, &mut files)?;
    let mut fences = Vec::new();
    for file in files {
        let md = fs::read_to_string(&file)
            .with_context(|| format!("read article {}", file.display()))?;
        fences.extend(wq_fences_in_file(&file, &md));
    }
    Ok(fences)
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
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

fn wq_fences_in_file(file: &Path, md: &str) -> Vec<WqFence> {
    let mut fences = Vec::new();
    let mut current_info: Option<String> = None;
    let mut current_code = String::new();

    for event in Parser::new(md) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let info = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(info) => info.to_string(),
                    pulldown_cmark::CodeBlockKind::Indented => String::new(),
                };
                current_info = info.starts_with("wq").then_some(info);
                current_code.clear();
            }
            Event::Text(text) if current_info.is_some() => {
                current_code.push_str(&text);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(info) = current_info.take() {
                    fences.push(WqFence {
                        file: file.to_path_buf(),
                        info,
                        code: current_code.trim_end().to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    fences
}
