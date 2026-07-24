use crate::wqdb::command::{ParsedCommand, ParsedLine, StopHookCommand, parse_line};
use crate::wqdb::host::Host;
use crate::wqdb::render::{enabled_marker, render_table};

pub(super) fn execute(host: &mut Host<'_, '_>, command: StopHookCommand<'_>) -> Result<(), String> {
    match command {
        StopHookCommand::Add { command } => add(host, command),
        StopHookCommand::List => {
            print(host);
            Ok(())
        }
        StopHookCommand::Delete { id } => {
            delete(host, id);
            Ok(())
        }
        StopHookCommand::Clear => {
            host.clear_stop_hooks();
            wqdb_println!(host, "cleared stop hooks");
            Ok(())
        }
    }
}

fn add(host: &mut Host<'_, '_>, command: &str) -> Result<(), String> {
    validate_command(command).map_err(|error| format!("invalid stop-hook command: {error}"))?;
    let hook = host.add_stop_hook(command.to_string());
    wqdb_println!(host, format!("stop hook #{} added", hook.id));
    Ok(())
}

fn validate_command(command: &str) -> Result<(), String> {
    match parse_line(command) {
        Ok(ParsedLine::Empty) => Err("command is empty".to_string()),
        Ok(ParsedLine::Command(ParsedCommand::StopHook(StopHookCommand::Add { command }))) => {
            validate_command(command)
        }
        Ok(ParsedLine::Command(_)) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn delete(host: &mut Host<'_, '_>, id: usize) {
    if host.remove_stop_hook(id) {
        wqdb_println!(host, format!("removed stop hook {id}"));
    } else {
        wqdb_println!(host, format!("stop hook {id} not found"));
    }
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

#[cfg(test)]
mod tests {
    use super::validate_command;

    #[test]
    fn stop_hook_commands_are_validated_recursively() {
        assert_eq!(validate_command("track list"), Ok(()));
        assert_eq!(
            validate_command("stop-hook add c ignored"),
            Err("unexpected argument 'ignored'; usage: continue".to_string())
        );
        assert_eq!(
            validate_command("stop-hook add stop-hook add -o c"),
            Err(
                "syntax 'stop-hook add -o <command>' is no longer supported; use 'stop-hook add <command...>'"
                    .to_string()
            )
        );
    }
}
