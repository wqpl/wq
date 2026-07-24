use crate::wqdb::command::{ParsedStopHook, Usage, usage_error};
use crate::wqdb::host::Host;
use crate::wqdb::render::{enabled_marker, render_table};

pub(super) fn execute(host: &mut Host<'_, '_>, command: ParsedStopHook<'_>) -> Result<(), String> {
    match command {
        ParsedStopHook::Add { command } => add(host, command),
        ParsedStopHook::List => {
            print(host);
            Ok(())
        }
        ParsedStopHook::Delete { target } => delete(host, target),
        ParsedStopHook::Clear => {
            host.clear_stop_hooks();
            wqdb_println!(host, "cleared stop hooks");
            Ok(())
        }
        ParsedStopHook::Invalid => Err(usage_error(Usage::StopHook)),
    }
}

fn add(host: &mut Host<'_, '_>, command: Option<&str>) -> Result<(), String> {
    let command = command.ok_or_else(|| usage_error(Usage::StopHookAdd))?;
    if command.is_empty() {
        return Err(usage_error(Usage::StopHookAdd));
    }
    let hook = host.add_stop_hook(command.to_string());
    wqdb_println!(host, format!("stop hook #{} added", hook.id));
    Ok(())
}

fn delete(host: &mut Host<'_, '_>, target: Option<&str>) -> Result<(), String> {
    let Some(target) = target else {
        return Err(usage_error(Usage::StopHookDelete));
    };
    if target == "all" {
        host.clear_stop_hooks();
        wqdb_println!(host, "cleared stop hooks");
        return Ok(());
    }
    let id = target
        .parse::<usize>()
        .map_err(|_| usage_error(Usage::StopHookDelete))?;
    if host.remove_stop_hook(id) {
        wqdb_println!(host, format!("removed stop hook {id}"));
    } else {
        wqdb_println!(host, format!("stop hook {id} not found"));
    }
    Ok(())
}

fn print(host: &Host<'_, '_>) {
    let hooks = host.stop_hooks();
    if hooks.is_empty() {
        wqdb_println!(host, "no stop hooks");
        return;
    }
    let rows = hooks
        .iter()
        .map(|hook| {
            vec![
                hook.id.to_string(),
                enabled_marker(hook.enabled).to_string(),
                hook.command.clone(),
            ]
        })
        .collect::<Vec<_>>();
    wqdb_println!(
        host,
        render_table(&["id", "en", "command"], &rows, &[4, 3, 20])
    );
}
