use wq_dap::r#type::{
    Breakpoint, Scope, ScopePresentationhint, Source, StackFrame, Thread, Variable,
};
use wqpl::debug::{CrashFrame, Debugger, SourceBreakpoint};
use wqpl::value::Excerpt;

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
        .map(|breakpoint| build_source_breakpoint(debugger, breakpoint))
        .collect()
}

pub(crate) fn build_source_breakpoint(
    debugger: &Debugger<'_>,
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

    let (line, column) = debugger
        .resolve_location(location)
        .and_then(|resolved| resolved.source)
        .map(|source| (Some(source.line as i64), Some(source.column as i64)))
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
    let frames = frames
        .iter()
        .enumerate()
        .map(|(id, frame)| crash_frame_to_stack_frame(debugger, frame, id))
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

fn crash_frame_to_stack_frame(
    debugger: &Debugger<'_>,
    frame: &CrashFrame,
    id: usize,
) -> StackFrame {
    let source = match frame {
        CrashFrame::Located {
            location, source, ..
        } => source.clone().or_else(|| {
            debugger
                .resolve_location(*location)
                .and_then(|resolved| resolved.source)
        }),
        CrashFrame::TailCallsOmitted => None,
    };
    let (source, line, column) = source
        .map(|source| {
            (
                Some(Source {
                    path: Some(source.path.to_string()),
                    ..Default::default()
                }),
                source.line as i64,
                source.column as i64,
            )
        })
        .unwrap_or((None, 0, 0));

    StackFrame {
        id: id as i64,
        name: frame.function().to_string(),
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

    let mut scopes = Vec::new();
    if debugger.frame_locals(frame_id).is_some() {
        scopes.push(Scope {
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
        });
    }

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
                type_field: Some(value.debug_kind().to_string()),
                variables_reference: 0,
                ..Default::default()
            })
            .collect()
    } else if let Some(frame_id) = decode_locals_ref(variables_reference) {
        if let Some(frame) = debugger.frame_locals(frame_id) {
            let local_names = debugger.local_names(frame.loc.chunk);
            frame
                .locals
                .iter()
                .map(|(slot, value)| {
                    let name = local_names
                        .and_then(|names| names.get(*slot).cloned())
                        .unwrap_or_else(|| format!("loc[{slot}]"));
                    Variable {
                        name,
                        value: value.excerpt(),
                        type_field: Some(value.debug_kind().to_string()),
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
    use std::sync::Arc;

    use wqpl::debug::{ChunkId, CodeLoc};
    use wqpl::session::Session;

    use super::*;

    #[test]
    fn tail_call_marker_is_a_source_less_stack_frame() {
        let mut session = Session::new();
        let debugger = session.debugger();
        let frame = crash_frame_to_stack_frame(&debugger, &CrashFrame::TailCallsOmitted, 2);

        assert_eq!(frame.id, 2);
        assert_eq!(frame.name, "(... tail calls omitted ...)");
        assert!(frame.source.is_none());
        assert_eq!(frame.line, 0);
        assert_eq!(frame.column, 0);
    }

    #[test]
    fn stack_frame_tolerates_missing_debug_metadata() {
        let mut session = Session::new();
        let debugger = session.debugger();
        let crash_frame = CrashFrame::Located {
            function: Arc::from("f"),
            location: CodeLoc {
                chunk: ChunkId(u32::MAX),
                pc: usize::MAX,
            },
            source: None,
            locals: None,
        };

        let frame = crash_frame_to_stack_frame(&debugger, &crash_frame, 0);

        assert_eq!(frame.name, "f");
        assert!(frame.source.is_none());
        assert_eq!(frame.line, 0);
        assert_eq!(frame.column, 0);
    }

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
