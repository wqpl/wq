use wqpl::value::Excerpt;
use wqpl::wqdb::{SymbolMutation, SymbolTrackTarget, TrackResult};

use crate::wqdb::command::{Command, TrackScope, Usage, usage_error};
use crate::wqdb::host::Host;
use crate::wqdb::render::{enabled_marker, render_table};

pub(super) fn track_symbol(
    host: &mut Host<'_, '_>,
    target_arg: Option<&str>,
    name_arg: Option<&str>,
) -> Result<(), String> {
    let Some(target_arg) = target_arg else {
        return Err(usage_error(Usage::Command(Command::Track)));
    };
    let result = if let Some(name_arg) = name_arg {
        match TrackScope::parse(target_arg) {
            Some(TrackScope::Global) => host.track_global_symbol(name_arg),
            Some(TrackScope::Local) => host
                .track_local_symbol(name_arg)
                .map_err(|error| error.to_string())?,
            Some(TrackScope::Capture) => {
                let slot = name_arg
                    .parse::<u16>()
                    .map_err(|_| usage_error(Usage::TrackCapture))?;
                host.track_capture_slot(slot)
                    .map_err(|error| error.to_string())?
            }
            None => return Err(usage_error(Usage::Command(Command::Track))),
        }
    } else {
        host.track_symbol(target_arg)
            .map_err(|error| error.to_string())?
    };
    if let TrackResult::Added(tracker) = result {
        wqdb_println!(
            host,
            format!(
                "tracking #{} {}",
                tracker.id,
                format_symbol_track_target(host, &tracker.target)
            )
        );
    }
    Ok(())
}

pub(super) fn untrack_symbol(host: &mut Host<'_, '_>, arg: Option<&str>) -> Result<(), String> {
    let Some(arg) = arg else {
        return Err(usage_error(Usage::Command(Command::Untrack)));
    };
    if arg == "all" {
        host.clear_symbol_trackers();
        wqdb_println!(host, "cleared symbol trackers");
        return Ok(());
    }
    let id = arg
        .parse::<usize>()
        .map_err(|_| usage_error(Usage::Command(Command::Untrack)))?;
    if host.remove_symbol_tracker(id) {
        wqdb_println!(host, format!("removed symbol tracker {id}"));
    } else {
        wqdb_println!(host, format!("symbol tracker {id} not found"));
    }
    Ok(())
}

pub(super) fn print_symbol_trackers(host: &Host<'_, '_>) {
    let trackers = host.symbol_trackers();
    if trackers.is_empty() {
        wqdb_println!(host, "no symbol trackers");
        return;
    }
    let rows = trackers
        .iter()
        .map(|tracker| {
            vec![
                tracker.id.to_string(),
                enabled_marker(tracker.enabled).to_string(),
                format_symbol_track_target(host, &tracker.target),
            ]
        })
        .collect::<Vec<_>>();
    wqdb_println!(
        host,
        render_table(&["id", "en", "target"], &rows, &[4, 3, 20])
    );
}

fn format_symbol_track_target(host: &Host<'_, '_>, target: &SymbolTrackTarget) -> String {
    match target {
        SymbolTrackTarget::Global { name } => format!("global {name}"),
        SymbolTrackTarget::Local { chunk, slot, name } => {
            format!("local {name} ({} slot {slot})", host.function_name(*chunk))
        }
        SymbolTrackTarget::Capture { chunk, slot, name } => match name {
            Some(name) => {
                format!(
                    "capture {name} ({} slot {slot})",
                    host.function_name(*chunk)
                )
            }
            None => format!("capture slot {slot} ({})", host.function_name(*chunk)),
        },
    }
}

pub(in crate::wqdb) fn print_symbol_mutation(host: &Host<'_, '_>, mutation: &SymbolMutation) {
    let target = format_symbol_track_target(host, &mutation.target);
    let location = host
        .debug_info()
        .resolve_location(mutation.location)
        .and_then(|resolved| {
            resolved.source.map(|source| {
                format!(
                    "{}:{}:{} in {}",
                    source.path, source.line, source.column, resolved.function
                )
            })
        })
        .unwrap_or_else(|| {
            format!(
                "pc {} in {}",
                mutation.location.pc,
                host.function_name(mutation.location.chunk)
            )
        });
    let old = mutation.old_value.as_ref().map_or_else(
        || "<unbound>".to_string(),
        |value| format!("{} ({})", value.excerpt(), value.debug_kind()),
    );
    let new = format!(
        "{} ({})",
        mutation.new_value.excerpt(),
        mutation.new_value.debug_kind()
    );
    wqdb_println!(
        host,
        format!(
            "[wqdb:track #{}] {target} {} at {location}: {old} -> {new}",
            mutation.tracker_id,
            mutation.operation.as_str()
        )
    );
}
