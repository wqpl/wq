use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender};

use wq_dap::event::Event;
use wq_dap::r#type::{Breakpoint, Scope, StackFrame, StoppedEventReason, Variable};
use wqpl::session::Session;
use wqpl::vm::Vm;

use crate::dap::adapter;
use crate::load::load_script;

thread_local! {
    static CMD_RX: RefCell<Option<Receiver<VmCommand>>> = const { RefCell::new(None) };
    static EVENT_TX: RefCell<Option<Sender<Event>>> = const { RefCell::new(None) };
}

pub(crate) enum VmCommand {
    Continue,
    StepIn,
    StepOver,
    StepOut,
    SetBreakpoints {
        source_path: String,
        lines: Vec<usize>,
        tx: Sender<Vec<Breakpoint>>,
    },
    StackTrace {
        start_frame: Option<usize>,
        levels: Option<usize>,
        tx: Sender<Vec<StackFrame>>,
    },
    Scopes {
        frame_id: usize,
        tx: Sender<Vec<Scope>>,
    },
    Variables {
        variables_reference: usize,
        tx: Sender<Vec<Variable>>,
    },
}

pub(crate) fn run_vm(script_path: &str, event_tx: Sender<Event>, cmd_rx: Receiver<VmCommand>) {
    CMD_RX.with(|rx| *rx.borrow_mut() = Some(cmd_rx));
    EVENT_TX.with(|tx| *tx.borrow_mut() = Some(event_tx.clone()));

    let mut session = Session::new();
    session.set_wqdb(true);
    session.set_pause_callback(Some(dap_on_pause));

    let loading = RefCell::new(HashSet::new());
    let exit_code = match load_script(&mut session, script_path, &loading, true) {
        Ok(_) => 0,
        Err(err) => {
            eprintln!("[wq-dap] load error: {err:?}");
            1
        }
    };

    let _ = event_tx.send(Event::Exited(wq_dap::event::ExitedEventBody { exit_code }));
}

fn dap_on_pause(vm: &mut Vm) {
    let loc = vm.loc();
    let di = vm.debug_info();
    let meta = di.chunk(loc.chunk);
    let span = meta.line_table.context_span_at(loc.pc);

    let (reason, description) = if span.file_id != u32::MAX && vm.wqdb.pause_loc().is_some() {
        if vm.wqdb.breaks.contains_key(&loc) {
            (
                StoppedEventReason::Breakpoint,
                Some("hit breakpoint".to_string()),
            )
        } else {
            (StoppedEventReason::Step, Some("single step".to_string()))
        }
    } else {
        (StoppedEventReason::Entry, Some("entry".to_string()))
    };

    EVENT_TX.with(|tx| {
        if let Some(ref tx) = *tx.borrow() {
            let _ = tx.send(Event::Stopped(wq_dap::event::StoppedEventBody {
                reason,
                description,
                thread_id: Some(1),
                all_threads_stopped: Some(true),
                preserve_focus_hint: None,
                text: None,
                hit_breakpoint_ids: None,
            }));
        }
    });

    loop {
        let cmd = CMD_RX.with(|rx| rx.borrow_mut().as_mut().and_then(|rx| rx.recv().ok()));
        match cmd {
            Some(VmCommand::Continue) => {
                vm.dbg_continue();
                break;
            }
            Some(VmCommand::StepIn) => {
                vm.dbg_step_in();
                break;
            }
            Some(VmCommand::StepOver) => {
                vm.dbg_step_over();
                break;
            }
            Some(VmCommand::StepOut) => {
                vm.dbg_step_out();
                break;
            }
            Some(VmCommand::SetBreakpoints {
                source_path,
                lines,
                tx,
            }) => {
                let bps = adapter::set_breakpoints(vm, &source_path, &lines);
                let _ = tx.send(bps);
            }
            Some(VmCommand::StackTrace {
                start_frame,
                levels,
                tx,
            }) => {
                let frames = adapter::build_stack_trace(vm, start_frame, levels);
                let _ = tx.send(frames);
            }
            Some(VmCommand::Scopes { frame_id, tx }) => {
                let scopes = adapter::build_scopes(vm, frame_id);
                let _ = tx.send(scopes);
            }
            Some(VmCommand::Variables {
                variables_reference,
                tx,
            }) => {
                let vars = adapter::build_variables(vm, variables_reference);
                let _ = tx.send(vars);
            }
            None => {
                vm.dbg_continue();
                break;
            }
        }
    }
}
