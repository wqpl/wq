use wqpl::value::Excerpt;
use wqpl::wqdb::{SymbolMutation, SymbolTrackTarget, TrackResult};

use crate::wqdb::command::{TrackCommand, TrackTarget};
use crate::wqdb::host::Host;
use crate::wqdb::render::{enabled_marker, render_table};

pub(super) fn execute(host: &mut Host<'_, '_>, command: TrackCommand<'_>) -> Result<(), String> {
    match command {
        TrackCommand::Add(target) => add(host, target),
        TrackCommand::List => {
            print_symbol_trackers(host);
            Ok(())
        }
        TrackCommand::Delete { id } => {
            if host.remove_symbol_tracker(id) {
                wqdb_println!(host, format!("removed symbol tracker {id}"));
            } else {
                wqdb_println!(host, format!("symbol tracker {id} not found"));
            }
            Ok(())
        }
        TrackCommand::Clear => {
            host.clear_symbol_trackers();
            wqdb_println!(host, "cleared symbol trackers");
            Ok(())
        }
    }
}

fn add(host: &mut Host<'_, '_>, target: TrackTarget<'_>) -> Result<(), String> {
    let result = match target {
        TrackTarget::Global(name) => host.track_global_symbol(name),
        TrackTarget::Local(name) => host
            .track_local_symbol(name)
            .map_err(|error| error.to_string())?,
        TrackTarget::Capture(slot) => host
            .track_capture_slot(slot)
            .map_err(|error| error.to_string())?,
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

fn print_symbol_trackers(host: &Host<'_, '_>) {
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
