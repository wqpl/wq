pub mod adapter;
pub mod thread;

use std::io::{BufReader, BufWriter, Stdout};
use std::path::PathBuf;

use wq_dap::prelude::*;
use wq_dap::request::Command;
use wq_dap::r#type::Capabilities;

use crate::dap::thread::{VmCommand, run_vm};

pub(crate) fn run_dap(script: Option<PathBuf>) {
    let stdin: BufReader<std::io::Stdin> = BufReader::new(std::io::stdin());
    let stdout: BufWriter<Stdout> = BufWriter::new(std::io::stdout());
    let mut server = wq_dap::server::Server::new(stdin, stdout);

    let mut vm_cmd_tx: Option<std::sync::mpsc::Sender<VmCommand>> = None;
    let mut vm_handle: Option<std::thread::JoinHandle<()>> = None;
    let mut _initialized = false;
    let mut script_path = script.map(|p| p.to_string_lossy().to_string());

    // Spawn a dedicated thread to forward VM events so the main thread
    // stays responsive to DAP requests while the VM is running.
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let server_output = server.output.clone();
    std::thread::spawn(move || {
        while let Ok(event) = event_rx.recv() {
            let mut out = server_output.lock().unwrap();
            if out.send_event(event).is_err() {
                break;
            }
        }
    });

    loop {
        let req = match server.poll_request() {
            Ok(Some(r)) => r,
            Ok(None) => break,
            Err(e) => {
                eprintln!("[wq-dap] read error: {e}");
                break;
            }
        };

        match req.command {
            Command::Initialize(_) => {
                let rsp = req.success(ResponseBody::Initialize(Capabilities {
                    supports_configuration_done_request: Some(true),
                    supports_function_breakpoints: Some(false),
                    supports_conditional_breakpoints: Some(false),
                    supports_hit_conditional_breakpoints: Some(false),
                    supports_evaluate_for_hovers: Some(false),
                    supports_step_back: Some(false),
                    supports_set_variable: Some(false),
                    supports_restart_frame: Some(false),
                    supports_goto_targets_request: Some(false),
                    supports_step_in_targets_request: Some(false),
                    supports_completions_request: Some(false),
                    supports_modules_request: Some(false),
                    supports_restart_request: Some(false),
                    supports_exception_options: Some(false),
                    supports_value_formatting_options: Some(false),
                    supports_exception_info_request: Some(false),
                    support_terminate_debuggee: Some(true),
                    support_suspend_debuggee: Some(false),
                    supports_delayed_stack_trace_loading: Some(false),
                    supports_loaded_sources_request: Some(false),
                    supports_log_points: Some(false),
                    supports_terminate_threads_request: Some(false),
                    supports_set_expression: Some(false),
                    supports_terminate_request: Some(true),
                    supports_data_breakpoints: Some(false),
                    supports_read_memory_request: Some(false),
                    supports_write_memory_request: Some(false),
                    supports_disassemble_request: Some(false),
                    supports_cancel_request: Some(false),
                    supports_breakpoint_locations_request: Some(false),
                    supports_clipboard_context: Some(false),
                    supports_stepping_granularity: Some(false),
                    supports_instruction_breakpoints: Some(false),
                    supports_exception_filter_options: Some(false),
                    supports_single_thread_execution_requests: Some(false),
                    ..Default::default()
                }));
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
                if let Err(e) = server.send_event(Event::Initialized) {
                    eprintln!("[wq-dap] event error: {e}");
                    break;
                }
                _initialized = true;
            }
            Command::Launch(ref args) => {
                // Extract program path from additional_data
                if script_path.is_none()
                    && let Some(ref data) = args.additional_data
                    && let Some(program) = data.get("program").and_then(|v| v.as_str())
                {
                    script_path = Some(program.to_string());
                }
                let path = script_path
                    .clone()
                    .unwrap_or_else(|| "<unknown>".to_string());

                let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
                vm_cmd_tx = Some(cmd_tx);

                let path_clone = path.clone();
                let event_tx2 = event_tx.clone();
                vm_handle = Some(std::thread::spawn(move || {
                    run_vm(&path_clone, event_tx2, cmd_rx);
                }));

                let rsp = req.success(ResponseBody::Launch);
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }

                // Also send a Process event
                let mut out = server.output.lock().unwrap();
                let _ = out.send_event(Event::Process(wq_dap::event::ProcessEventBody {
                    name: path,
                    start_method: Some(wq_dap::r#type::ProcessEventStartMethod::Launch),
                    ..Default::default()
                }));
            }
            Command::SetBreakpoints(ref args) => {
                let source_path = args.source.path.clone().unwrap_or_default();
                let lines: Vec<usize> = args
                    .breakpoints
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|b| b.line as usize)
                    .collect();

                let bps = if let Some(ref tx) = vm_cmd_tx {
                    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
                    let _ = tx.send(VmCommand::SetBreakpoints {
                        source_path,
                        lines,
                        tx: reply_tx,
                    });
                    reply_rx.recv().unwrap_or_default()
                } else {
                    Vec::new()
                };

                let rsp = req.success(ResponseBody::SetBreakpoints(
                    wq_dap::response::SetBreakpointsResponse { breakpoints: bps },
                ));
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
            }
            Command::StackTrace(ref args) => {
                let frames = if let Some(ref tx) = vm_cmd_tx {
                    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
                    let _ = tx.send(VmCommand::StackTrace {
                        start_frame: args.start_frame.map(|v| v as usize),
                        levels: args.levels.map(|v| v as usize),
                        tx: reply_tx,
                    });
                    reply_rx.recv().unwrap_or_default()
                } else {
                    Vec::new()
                };
                let total = frames.len() as i64;
                let rsp = req.success(ResponseBody::StackTrace(
                    wq_dap::response::StackTraceResponse {
                        stack_frames: frames,
                        total_frames: Some(total),
                    },
                ));
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
            }
            Command::Scopes(ref args) => {
                let scopes = if let Some(ref tx) = vm_cmd_tx {
                    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
                    let _ = tx.send(VmCommand::Scopes {
                        frame_id: args.frame_id as usize,
                        tx: reply_tx,
                    });
                    reply_rx.recv().unwrap_or_default()
                } else {
                    Vec::new()
                };
                let rsp = req.success(ResponseBody::Scopes(wq_dap::response::ScopesResponse {
                    scopes,
                }));
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
            }
            Command::Variables(ref args) => {
                let vars = if let Some(ref tx) = vm_cmd_tx {
                    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
                    let _ = tx.send(VmCommand::Variables {
                        variables_reference: args.variables_reference as usize,
                        tx: reply_tx,
                    });
                    reply_rx.recv().unwrap_or_default()
                } else {
                    Vec::new()
                };
                let rsp = req.success(ResponseBody::Variables(
                    wq_dap::response::VariablesResponse { variables: vars },
                ));
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
            }
            Command::Continue(_) => {
                if let Some(ref tx) = vm_cmd_tx {
                    let _ = tx.send(VmCommand::Continue);
                }
                let rsp = req.success(ResponseBody::Continue(wq_dap::response::ContinueResponse {
                    all_threads_continued: Some(true),
                }));
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
            }
            Command::Next(_) => {
                if let Some(ref tx) = vm_cmd_tx {
                    let _ = tx.send(VmCommand::StepOver);
                }
                let rsp = req.success(ResponseBody::Next);
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
            }
            Command::StepIn(_) => {
                if let Some(ref tx) = vm_cmd_tx {
                    let _ = tx.send(VmCommand::StepIn);
                }
                let rsp = req.success(ResponseBody::StepIn);
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
            }
            Command::StepOut(_) => {
                if let Some(ref tx) = vm_cmd_tx {
                    let _ = tx.send(VmCommand::StepOut);
                }
                let rsp = req.success(ResponseBody::StepOut);
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
            }
            Command::Threads => {
                let threads = adapter::build_threads();
                let rsp = req.success(ResponseBody::Threads(wq_dap::response::ThreadsResponse {
                    threads,
                }));
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
            }
            Command::Disconnect(ref args) => {
                if args.terminate_debuggee.unwrap_or(true) {
                    drop(vm_cmd_tx);
                    if let Some(handle) = vm_handle.take() {
                        let _ = handle.join();
                    }
                }
                let rsp = req.success(ResponseBody::Disconnect);
                // Ignore I/O errors here: the client may have already closed
                // the connection after requesting disconnect.
                let _ = server.respond(rsp);
                break;
            }
            Command::ConfigurationDone => {
                let rsp = req.success(ResponseBody::ConfigurationDone);
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
            }
            _ => {
                let rsp = req.error("not supported");
                if let Err(e) = server.respond(rsp) {
                    eprintln!("[wq-dap] respond error: {e}");
                    break;
                }
            }
        }
    }
}
