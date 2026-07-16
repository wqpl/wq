use wq_dap::r#type::{
    Breakpoint, Scope, ScopePresentationhint, Source, StackFrame, Thread, Variable,
};
use wqpl::debugger::Debugger;
use wqpl::value::Excerpt;
use wqpl::wqdb::data::{CodeLoc, DebugInfo};
use wqpl::wqdb::model::SourceBreakpoint;

pub(crate) struct StackTracePage {
    pub(crate) frames: Vec<StackFrame>,
    pub(crate) total_frames: usize,
}

/// Replace the source breakpoints tracked by the debuggee.
pub(crate) fn set_breakpoints(
    debugger: &mut Debugger<'_>,
    source_path: &str,
    lines: &[usize],
) -> Vec<Breakpoint> {
    debugger
        .set_source_breakpoints(source_path, lines)
        .iter()
        .map(|breakpoint| build_source_breakpoint(debugger.debug_info(), breakpoint))
        .collect()
}

pub(crate) fn build_source_breakpoint(
    debug_info: &DebugInfo,
    breakpoint: &SourceBreakpoint,
) -> Breakpoint {
    let source = Some(Source {
        path: Some(breakpoint.source_path.clone()),
        ..Default::default()
    });
    let Some(location) = breakpoint.location else {
        return Breakpoint {
            id: Some(breakpoint.id as i64),
            verified: false,
            message: Some(format!(
                "line {} has not been compiled yet",
                breakpoint.requested_line
            )),
            source,
            line: Some(breakpoint.requested_line as i64),
            ..Default::default()
        };
    };

    let (line, column) = debug_info
        .chunk_opt(location.chunk)
        .and_then(|chunk| {
            let span = chunk.line_table.span_at(location.pc);
            debug_info.file(span.file_id).map(|file| {
                let (line, column) = file.line_col(span.start);
                (Some(line as i64), Some(column as i64))
            })
        })
        .unwrap_or((Some(breakpoint.requested_line as i64), None));
    Breakpoint {
        id: Some(breakpoint.id as i64),
        verified: true,
        source,
        line,
        column,
        ..Default::default()
    }
}

pub(crate) fn build_stack_trace(
    debugger: &Debugger<'_>,
    start_frame: Option<usize>,
    levels: Option<usize>,
) -> StackTracePage {
    let frames = debugger.backtrace();
    let di = debugger.debug_info();
    let frames = frames
        .iter()
        .enumerate()
        .map(|(id, (loc, name))| loc_to_stack_frame(di, *loc, name.as_ref(), id))
        .collect();
    paginate_stack_frames(frames, start_frame, levels)
}

fn paginate_stack_frames(
    frames: Vec<StackFrame>,
    start_frame: Option<usize>,
    levels: Option<usize>,
) -> StackTracePage {
    let total_frames = frames.len();
    let start = start_frame.unwrap_or(0).min(total_frames);
    let available = total_frames - start;
    let requested = levels.filter(|levels| *levels != 0).unwrap_or(available);
    let frames = frames
        .into_iter()
        .skip(start)
        .take(requested.min(available))
        .collect();
    StackTracePage {
        frames,
        total_frames,
    }
}

fn loc_to_stack_frame(di: &DebugInfo, loc: CodeLoc, name: &str, id: usize) -> StackFrame {
    let meta = di.chunk_opt(loc.chunk);
    let (source, line, column) = meta
        .and_then(|m| {
            let span = m.line_table.span_at(loc.pc);
            if span.file_id == u32::MAX {
                let ctx = m.line_table.context_span_at(loc.pc);
                if ctx.file_id == u32::MAX {
                    return None;
                }
                di.file(ctx.file_id).map(|sf| {
                    let (l, c) = sf.line_col(ctx.start);
                    (
                        Some(Source {
                            path: Some(sf.path.to_string()),
                            ..Default::default()
                        }),
                        l as i64,
                        c as i64,
                    )
                })
            } else {
                di.file(span.file_id).map(|sf| {
                    let (l, c) = sf.line_col(span.start);
                    (
                        Some(Source {
                            path: Some(sf.path.to_string()),
                            ..Default::default()
                        }),
                        l as i64,
                        c as i64,
                    )
                })
            }
        })
        .unwrap_or((None, 0, 0));

    StackFrame {
        id: id as i64,
        name: name.to_string(),
        source,
        line,
        column,
        ..Default::default()
    }
}

pub(crate) fn build_scopes(debugger: &Debugger<'_>, frame_id: usize) -> Vec<Scope> {
    let frames = debugger.backtrace();
    if frame_id >= frames.len() {
        return Vec::new();
    }

    let mut scopes = vec![Scope {
        name: "Locals".to_string(),
        presentation_hint: Some(ScopePresentationhint::Locals),
        variables_reference: locals_ref(frame_id),
        named_variables: None,
        indexed_variables: None,
        expensive: false,
        source: None,
        line: None,
        column: None,
        end_line: None,
        end_column: None,
    }];

    if frame_id == 0 {
        scopes.push(Scope {
            name: "Globals".to_string(),
            presentation_hint: None,
            variables_reference: globals_ref(),
            named_variables: None,
            indexed_variables: None,
            expensive: false,
            source: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
        });
    }

    scopes
}

pub(crate) fn build_variables(
    debugger: &Debugger<'_>,
    variables_reference: usize,
) -> Vec<Variable> {
    if variables_reference == globals_ref() as usize {
        debugger
            .globals()
            .into_iter()
            .map(|(name, value)| Variable {
                name,
                value: value.excerpt(),
                type_field: Some(value.type_name().to_string()),
                variables_reference: 0,
                ..Default::default()
            })
            .collect()
    } else if let Some(frame_id) = decode_locals_ref(variables_reference) {
        let all_frames = debugger.local_frames();
        if let Some(frame) = all_frames.get(frame_id) {
            let meta = debugger.debug_info().chunk(frame.loc.chunk);
            frame
                .locals
                .iter()
                .map(|(slot, value)| {
                    let name = meta
                        .local_names
                        .as_ref()
                        .and_then(|names| names.get(*slot).cloned())
                        .unwrap_or_else(|| format!("loc[{slot}]"));
                    Variable {
                        name,
                        value: value.excerpt(),
                        type_field: Some(value.type_name().to_string()),
                        variables_reference: 0,
                        ..Default::default()
                    }
                })
                .collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    }
}

pub(crate) fn build_threads() -> Vec<Thread> {
    vec![Thread {
        id: 1,
        name: "main".to_string(),
    }]
}

// Variable reference encoding:
//   1          = globals
//   100 + id   = locals for frame id

const fn globals_ref() -> i64 {
    1
}

const fn locals_ref(frame_id: usize) -> i64 {
    100 + frame_id as i64
}

fn decode_locals_ref(variables_reference: usize) -> Option<usize> {
    if variables_reference >= 100 {
        Some(variables_reference - 100)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_trace_pagination_keeps_the_full_available_frame_count() {
        let frames = (0..3)
            .map(|id| StackFrame {
                id,
                name: format!("frame-{id}"),
                line: 1,
                column: 1,
                ..Default::default()
            })
            .collect();

        let page = paginate_stack_frames(frames, Some(1), Some(1));

        assert_eq!(page.frames.len(), 1);
        assert_eq!(page.frames[0].id, 1);
        assert_eq!(page.total_frames, 3);
    }
}
