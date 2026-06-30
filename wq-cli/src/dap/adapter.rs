use wq_dap::r#type::{
    Breakpoint, Scope, ScopePresentationhint, Source, StackFrame, Thread, Variable,
};
use wqpl::value::Excerpt;
use wqpl::vm::Vm;
use wqpl::wqdb::data::{CodeLoc, DebugInfo};

/// Resolve source lines to CodeLoc breakpoints and set them on the VM.
pub(crate) fn set_breakpoints(vm: &mut Vm, source_path: &str, lines: &[usize]) -> Vec<Breakpoint> {
    // A single source file may be registered multiple times (different
    // file_ids) because wq evaluates it in streaming chunks. Collect all
    // matching file_ids so breakpoints anywhere in the file resolve.
    let file_ids = vm.debug_info().file_ids_by_path(source_path);

    let mut result = Vec::with_capacity(lines.len());
    for &line in lines {
        let mut resolved = None;
        for &fid in &file_ids {
            let locs = vm.debug_info().resolve_line(fid, line);
            if let Some(&loc) = locs.first() {
                vm.dbg_set_break(loc);
                let (actual_line, actual_col) = if let Some(sf) = vm.debug_info().file(fid) {
                    let meta = vm.debug_info().chunk(loc.chunk);
                    let span = meta.line_table.span_at(loc.pc);
                    if span.file_id != u32::MAX {
                        let (l, c) = sf.line_col(span.start);
                        (Some(l as i64), Some(c as i64))
                    } else {
                        (Some(line as i64), None)
                    }
                } else {
                    (Some(line as i64), None)
                };
                resolved = Some((actual_line, actual_col));
                break;
            }
        }
        if let Some((actual_line, actual_col)) = resolved {
            result.push(Breakpoint {
                verified: true,
                source: Some(Source {
                    path: Some(source_path.to_string()),
                    ..Default::default()
                }),
                line: actual_line,
                column: actual_col,
                ..Default::default()
            });
        } else {
            // Unresolved breakpoint
            result.push(Breakpoint {
                verified: false,
                message: Some(format!("no statement at line {line}")),
                source: Some(Source {
                    path: Some(source_path.to_string()),
                    ..Default::default()
                }),
                line: Some(line as i64),
                ..Default::default()
            });
        }
    }
    result
}

pub(crate) fn build_stack_trace(
    vm: &Vm,
    start_frame: Option<usize>,
    levels: Option<usize>,
) -> Vec<StackFrame> {
    let frames = vm.bt_frames();
    let di = vm.debug_info();
    let start = start_frame.unwrap_or(0);
    let end = levels
        .map(|l| start + l)
        .unwrap_or(frames.len())
        .min(frames.len());

    frames
        .iter()
        .enumerate()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|(id, (loc, name))| loc_to_stack_frame(di, *loc, name.as_ref(), id))
        .collect()
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

pub(crate) fn build_scopes(vm: &Vm, frame_id: usize) -> Vec<Scope> {
    let frames = vm.bt_frames();
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

pub(crate) fn build_variables(vm: &Vm, variables_reference: usize) -> Vec<Variable> {
    if variables_reference == globals_ref() as usize {
        vm.dbg_globals()
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
        let all_frames = vm.dbg_local_frames();
        if let Some(frame) = all_frames.get(frame_id) {
            let meta = vm.debug_info().chunk(frame.loc.chunk);
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
