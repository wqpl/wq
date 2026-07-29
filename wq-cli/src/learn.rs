use std::fmt::Write as _;

use crate::{display, help, note};

pub(super) struct EmbeddedChapter {
    pub(super) slug: &'static str,
    pub(super) title: &'static str,
    pub(super) description: &'static str,
    pub(super) content: &'static str,
    pub(super) optional: bool,
}

include!(concat!(env!("OUT_DIR"), "/book.rs"));

pub(super) fn run(chapter: Option<&str>, list: bool, no_pager: bool) -> i32 {
    let fold_width = help::auto_fold_width(display::terminal_width());
    if list {
        let mut markdown = format!("# {BOOK_TITLE}\n\n{BOOK_DESCRIPTION}\n\n");
        markdown.push_str("| Chapter | What it covers |\n| --- | --- |\n");
        for item in BOOK_CHAPTERS {
            let optional = if item.optional { " (optional)" } else { "" };
            writeln!(
                markdown,
                "| `{}` | {}{}: {} |",
                item.slug, item.title, optional, item.description
            )
            .expect("writing the book index must succeed");
        }
        markdown.push_str("\nStart with `wq learn`. Open a chapter with `wq learn CHAPTER`.");
        note::run_markdown_content_with_fold_width(&markdown, no_pager, fold_width);
        return 0;
    }

    let requested = chapter.unwrap_or("start");
    let Some((index, item)) = BOOK_CHAPTERS
        .iter()
        .enumerate()
        .find(|(_, item)| item.slug.eq_ignore_ascii_case(requested))
    else {
        eprintln!(
            "Unknown book chapter '{requested}'. Run `wq learn --list` to see chapter names."
        );
        return 2;
    };

    let mut markdown = item.content.to_string();
    markdown.push_str("\n\n---\n\n");
    if let Some(previous) = index
        .checked_sub(1)
        .and_then(|previous| BOOK_CHAPTERS.get(previous))
    {
        writeln!(
            markdown,
            "Previous: `{}` with `wq learn {}`.  ",
            previous.title, previous.slug
        )
        .expect("writing previous chapter navigation must succeed");
    }
    if let Some(next) = BOOK_CHAPTERS.get(index + 1) {
        writeln!(
            markdown,
            "Next: `{}` with `wq learn {}`.",
            next.title, next.slug
        )
        .expect("writing next chapter navigation must succeed");
    } else {
        markdown.push_str("End of the introductory book.");
    }

    note::run_markdown_content_with_fold_width(&markdown, no_pager, fold_width);
    0
}
