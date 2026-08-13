use std::cell::RefCell;
use std::fmt::Write as _;

use crate::interpret::vanilla::{InterpretPoll, VanillaInterpreter};
use crate::interpret::{Interpreter, InterpreterHook, InterpreterKind};
use crate::session::stdio::WqIoError;
use crate::value::{Value, WqResult};
use crate::vm::Vm;
use crate::vm::inst::Instruction;
use crate::wqerror::{WqError, WqErrorType};

const WIDTH: usize = 48;
const HEIGHT: usize = 14;
const FRAME_INTERVAL: usize = 12;
const DECAY: u8 = 18;
const CAT_STAR_CHARS: [char; 3] = ['*', '•', '+'];

pub(crate) struct SampleInterpreter {
    art: RefCell<InstructionArt>,
    io_error: RefCell<Option<WqIoError>>,
    automatic: bool,
    running: bool,
}

impl Default for SampleInterpreter {
    fn default() -> Self {
        Self {
            art: RefCell::new(InstructionArt::new(RenderMode::FinalOnly, false)),
            io_error: RefCell::new(None),
            automatic: true,
            running: false,
        }
    }
}

impl Interpreter for SampleInterpreter {
    fn interpret(&mut self, vm: &mut Vm, limit: usize) -> WqResult<Value> {
        self.begin(vm);
        let result = self.with_hooks(vm, |delegate, vm| delegate.interpret(vm, limit));
        self.running = false;
        self.finish(vm, result)
    }
}

impl SampleInterpreter {
    fn begin(&mut self, vm: &Vm) {
        if self.automatic {
            *self.art.get_mut() = InstructionArt::auto(
                vm.stderr_is_terminal(),
                vm.stderr_color_mode().should_colorize(),
            );
        } else {
            self.art.get_mut().color = vm.stderr_color_mode().should_colorize();
        }
        *self.io_error.get_mut() = None;
        self.running = true;
    }

    fn with_hooks<T>(
        &mut self,
        vm: &mut Vm,
        run: impl FnOnce(&mut VanillaInterpreter, &mut Vm) -> WqResult<T>,
    ) -> WqResult<T> {
        let mut delegate = VanillaInterpreter;
        let previous_interpreter = vm.interpreter_kind;
        vm.interpreter_kind = InterpreterKind::Vanilla;
        vm.set_hooks(Some(self));
        let result = run(&mut delegate, vm);
        vm.set_hooks(None);
        vm.interpreter_kind = previous_interpreter;
        result
    }

    fn finish<T>(&mut self, vm: &Vm, result: WqResult<T>) -> WqResult<T> {
        if let Err(error) = self.art.get_mut().finish(vm) {
            *self.io_error.get_mut() = Some(error);
        }
        match (result, self.io_error.get_mut().take()) {
            (Err(error), _) => Err(error),
            (Ok(_), Some(error)) => Err(sample_io_error(error)),
            (Ok(value), None) => Ok(value),
        }
    }

    pub(crate) fn interpret_slice(
        &mut self,
        vm: &mut Vm,
        limit: usize,
        work_budget: usize,
    ) -> WqResult<InterpretPoll> {
        if !self.running {
            self.begin(vm);
        }
        let result = self.with_hooks(vm, |delegate, vm| {
            delegate.interpret_slice(vm, limit, work_budget)
        });
        if matches!(
            result,
            Ok(InterpretPoll::Yielded { .. }
                | InterpretPoll::AwaitingInput { .. }
                | InterpretPoll::Paused(_))
        ) {
            return result;
        }
        self.running = false;
        self.finish(vm, result)
    }
}

impl InterpreterHook for SampleInterpreter {
    fn requires_materialized_frames(&self) -> bool {
        true
    }

    fn before_instruction(&self, vm: &Vm, idx: usize, op: &Instruction) {
        if self.io_error.borrow().is_none()
            && let Err(error) = self.art.borrow_mut().observe(vm, idx, op)
        {
            *self.io_error.borrow_mut() = Some(error);
        }
    }
}

fn sample_io_error(error: WqIoError) -> WqError {
    WqError::new(WqErrorType::Io)
        .src("sample interpreter")
        .host_failure()
        .attach_note(format!("host I/O error: {error}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderMode {
    Off,
    FinalOnly,
    Animated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DotColor {
    Cyan,
    Green,
    Yellow,
    Magenta,
    White,
    Red,
}

const LEGEND: &[(&str, DotColor)] = &[
    ("L", DotColor::Cyan),
    ("S", DotColor::Green),
    ("O", DotColor::Yellow),
    ("C", DotColor::Magenta),
    ("J", DotColor::White),
    ("B", DotColor::Red),
    ("I", DotColor::Cyan),
    ("K", DotColor::White),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pixel {
    heat: u8,
    color: DotColor,
}

#[derive(Clone, Copy)]
struct Signal {
    label: &'static str,
    color: DotColor,
    strength: u8,
    radius: usize,
    salt: u64,
}

struct InstructionArt {
    pixels: [[Pixel; WIDTH]; HEIGHT],
    mode: RenderMode,
    color: bool,
    started: bool,
    ops: usize,
    frames: usize,
    max_stack_len: usize,
    max_call_depth: usize,
    last_pc: usize,
    last_label: &'static str,
}

impl InstructionArt {
    fn auto(stderr_is_terminal: bool, color: bool) -> Self {
        let setting = std::env::var("WQ_SAMPLE_ART").ok();
        Self::configured(setting.as_deref(), stderr_is_terminal, color)
    }

    fn configured(setting: Option<&str>, stderr_is_terminal: bool, color: bool) -> Self {
        let mode = match setting {
            Some("0" | "off" | "false" | "quiet") => RenderMode::Off,
            Some("static" | "final") => RenderMode::FinalOnly,
            Some("1" | "on" | "force" | "animate") => RenderMode::Animated,
            _ if stderr_is_terminal => RenderMode::Animated,
            _ => RenderMode::FinalOnly,
        };
        Self::new(mode, color)
    }

    fn new(mode: RenderMode, color: bool) -> Self {
        Self {
            pixels: [[Pixel::empty(); WIDTH]; HEIGHT],
            mode,
            color,
            started: false,
            ops: 0,
            frames: 0,
            max_stack_len: 0,
            max_call_depth: 0,
            last_pc: 0,
            last_label: "start",
        }
    }

    fn observe(&mut self, vm: &Vm, pc: usize, op: &Instruction) -> Result<(), WqIoError> {
        self.ops += 1;
        self.max_stack_len = self.max_stack_len.max(vm.stack.len());
        self.max_call_depth = self.max_call_depth.max(vm.physical_call_depth());
        self.last_pc = pc;

        let signal = signal_for(op);
        self.last_label = signal.label;
        if self.mode == RenderMode::Off {
            return Ok(());
        }

        self.fade();

        let seed = mix(signal.salt
            ^ usize_to_u64_hash(pc).wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ usize_to_u64_hash(self.ops).wrapping_mul(0xbf58_476d_1ce4_e5b9)
            ^ (usize_to_u64_hash(vm.stack.len()) << 32)
            ^ (usize_to_u64_hash(vm.physical_call_depth()) << 48));
        let x = hash_coord(seed, WIDTH);
        let y = hash_coord(seed >> 16, HEIGHT);
        self.paint(x, y, signal);

        if self.mode == RenderMode::Animated
            && (self.ops == 1 || self.ops.is_multiple_of(FRAME_INTERVAL))
        {
            self.render(vm)?;
        }
        Ok(())
    }

    fn finish(&mut self, vm: &Vm) -> Result<(), WqIoError> {
        if self.mode == RenderMode::Off || self.ops == 0 {
            return Ok(());
        }
        self.render(vm)?;
        if self.started {
            vm.write_stderr("\x1b[?25h\n")?;
        }
        Ok(())
    }

    fn fade(&mut self) {
        for row in &mut self.pixels {
            for pixel in row {
                pixel.heat = pixel.heat.saturating_sub(DECAY);
            }
        }
    }

    fn paint(&mut self, x: usize, y: usize, signal: Signal) {
        let radius = signal.radius as isize;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let distance = dx.unsigned_abs() + dy.unsigned_abs();
                if distance > signal.radius {
                    continue;
                }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx < 0 || ny < 0 || nx >= WIDTH as isize || ny >= HEIGHT as isize {
                    continue;
                }

                let falloff = u8::try_from(distance).unwrap_or(u8::MAX).saturating_mul(42);
                let heat = signal.strength.saturating_sub(falloff);
                if heat == 0 {
                    continue;
                }

                let nx = usize::try_from(nx).expect("x coordinate checked non-negative");
                let ny = usize::try_from(ny).expect("y coordinate checked non-negative");
                let pixel = &mut self.pixels[ny][nx];
                pixel.heat = pixel.heat.saturating_add(heat);
                pixel.color = signal.color;
            }
        }
    }

    fn render(&mut self, vm: &Vm) -> Result<(), WqIoError> {
        let frame = self.frame();
        let line_count = HEIGHT + 1;

        if self.mode == RenderMode::Animated {
            if self.started {
                vm.write_stderr(&format!("\x1b[{line_count}F"))?;
            } else {
                vm.write_stderr("\x1b[?25l")?;
                self.started = true;
            }
        }

        vm.write_stderr(&frame)?;
        self.frames += 1;
        Ok(())
    }

    fn frame(&self) -> String {
        let mut out = String::new();
        let control = self.mode == RenderMode::Animated;

        for row in &self.pixels {
            if control {
                out.push_str("\x1b[2K\r");
            }
            for pixel in row {
                self.push_pixel(&mut out, *pixel);
            }
            out.push('\n');
        }

        if control {
            out.push_str("\x1b[2K\r");
        }
        self.push_legend(&mut out);
        out.push('\n');
        out
    }

    fn push_pixel(&self, out: &mut String, pixel: Pixel) {
        let glyph = glyph_for_heat(pixel.heat);
        if !self.color || glyph == ' ' {
            out.push(glyph);
            return;
        }

        write!(
            out,
            "\x1b[{}m{}\x1b[0m",
            pixel.color.ansi_code(pixel.heat),
            glyph
        )
        .expect("write to string");
    }

    fn push_legend(&self, out: &mut String) {
        for (idx, (label, color)) in LEGEND.iter().enumerate() {
            if idx > 0 {
                out.push(' ');
            }
            if self.color {
                write!(out, "\x1b[{}m{label}\x1b[0m", color.ansi_code(u8::MAX))
                    .expect("write to string");
            } else {
                out.push_str(label);
            }
        }
    }
}

impl Pixel {
    const fn empty() -> Self {
        Self {
            heat: 0,
            color: DotColor::White,
        }
    }
}

impl DotColor {
    fn ansi_code(self, heat: u8) -> &'static str {
        match (self, heat >= 170) {
            (DotColor::Cyan, false) => "36",
            (DotColor::Cyan, true) => "96",
            (DotColor::Green, false) => "32",
            (DotColor::Green, true) => "92",
            (DotColor::Yellow, false) => "33",
            (DotColor::Yellow, true) => "93",
            (DotColor::Magenta, false) => "35",
            (DotColor::Magenta, true) => "95",
            (DotColor::White, false) => "37",
            (DotColor::White, true) => "97",
            (DotColor::Red, false) => "31",
            (DotColor::Red, true) => "91",
        }
    }
}

fn glyph_for_heat(heat: u8) -> char {
    match heat {
        0..=24 => ' ',
        25..=104 => CAT_STAR_CHARS[2],
        105..=184 => CAT_STAR_CHARS[0],
        _ => CAT_STAR_CHARS[1],
    }
}

fn signal_for(op: &Instruction) -> Signal {
    if is_call(op) {
        Signal {
            label: "call",
            color: DotColor::Magenta,
            strength: 210,
            radius: 2,
            salt: 0x5a17_5a17 ^ usize_to_u64_hash(instruction_amount(op)),
        }
    } else if is_jump(op) {
        Signal {
            label: "jump",
            color: DotColor::White,
            strength: 185,
            radius: 1,
            salt: 0x70ad_70ad ^ usize_to_u64_hash(instruction_amount(op)),
        }
    } else if is_build(op) {
        Signal {
            label: "build",
            color: DotColor::Red,
            strength: 230,
            radius: 2,
            salt: 0xb11d_b11d ^ usize_to_u64_hash(instruction_amount(op)),
        }
    } else if is_op(op) {
        Signal {
            label: "op",
            color: DotColor::Yellow,
            strength: 205,
            radius: 1,
            salt: 0x0f0f_0f0f ^ usize_to_u64_hash(instruction_amount(op)),
        }
    } else if is_store(op) {
        Signal {
            label: "store",
            color: DotColor::Green,
            strength: 190,
            radius: 1,
            salt: 0x570e_570e ^ usize_to_u64_hash(instruction_amount(op)),
        }
    } else if is_index(op) {
        Signal {
            label: "index",
            color: DotColor::Cyan,
            strength: 175,
            radius: 1,
            salt: 0x1d3c_1d3c ^ usize_to_u64_hash(instruction_amount(op)),
        }
    } else if is_load(op) {
        Signal {
            label: "load",
            color: DotColor::Cyan,
            strength: 160,
            radius: 0,
            salt: 0x10ad_10ad ^ usize_to_u64_hash(instruction_amount(op)),
        }
    } else {
        Signal {
            label: "stack",
            color: DotColor::White,
            strength: 140,
            radius: 0,
            salt: 0x57ac_57ac ^ usize_to_u64_hash(instruction_amount(op)),
        }
    }
}

fn is_load(op: &Instruction) -> bool {
    use Instruction as I;
    matches!(
        op,
        I::LoadConst(_)
            | I::LoadOwnedConst(_)
            | I::LoadClosure(_)
            | I::LoadVar(_)
            | I::LoadCallTarget(_)
            | I::LoadVarExists(_)
            | I::LoadCapture(_)
            | I::LoadSelf
            | I::LoadLocal(_)
    )
}

fn is_store(op: &Instruction) -> bool {
    use Instruction as I;
    matches!(
        op,
        I::StoreVar(_)
            | I::StoreVarKeep(_)
            | I::StoreLocal(_)
            | I::StoreLocalKeep(_)
            | I::StoreCapture(_)
            | I::StoreCaptureKeep(_)
            | I::CatAssign(_)
            | I::IndexAssignVar(_)
            | I::IndexAssignLocal(_)
            | I::IndexAssignCapture(_)
            | I::IndexManyAssignVar(_, _)
            | I::IndexManyAssignLocal(_, _)
            | I::IndexManyAssignCapture(_, _)
            | I::IndexAssignVarDrop(_)
            | I::IndexAssignLocalDrop(_)
            | I::IndexAssignCaptureDrop(_)
            | I::IndexManyAssignVarDrop(_, _)
            | I::IndexManyAssignLocalDrop(_, _)
            | I::IndexManyAssignCaptureDrop(_, _)
            | I::IndexMutate { .. }
    )
}

fn is_op(op: &Instruction) -> bool {
    use Instruction as I;
    matches!(
        op,
        I::BinaryOp(_) | I::UnaryOp(_) | I::CmpChain(_) | I::BoolCombine(_)
    )
}

fn is_jump(op: &Instruction) -> bool {
    use Instruction as I;
    matches!(
        op,
        I::Jump(_)
            | I::JumpIfFalse(_)
            | I::JumpIfCmpFalse(_)
            | I::NLoopEnter(_)
            | I::NLoopNext(_)
            | I::JumpIfGE(_)
            | I::JumpIfLEZLocal(_, _)
            | I::JumpIfNamedProvided(_, _, _)
            | I::BoolAndLazy(_)
            | I::BoolOrLazy(_)
    )
}

fn is_build(op: &Instruction) -> bool {
    use Instruction as I;
    matches!(
        op,
        I::Cat(_) | I::MakeList(_) | I::MakeDict(_) | I::MakeRange { .. } | I::LoadClosure(_)
    )
}

fn is_index(op: &Instruction) -> bool {
    use Instruction as I;
    matches!(
        op,
        I::Index
            | I::IndexMany(_)
            | I::IndexLoadLocal(_)
            | I::IndexLoadCapture(_)
            | I::IndexLoadVar(_)
            | I::IndexManyLoadLocal(_, _)
            | I::IndexManyLoadCapture(_, _)
            | I::IndexManyLoadVar(_, _)
            | I::IndexManyAssignLocal(_, _)
            | I::IndexManyAssignCapture(_, _)
            | I::IndexManyAssignVar(_, _)
            | I::IndexManyAssignLocalDrop(_, _)
            | I::IndexManyAssignCaptureDrop(_, _)
            | I::IndexManyAssignVarDrop(_, _)
    )
}

fn is_call(op: &Instruction) -> bool {
    use Instruction as I;
    matches!(
        op,
        I::CallBuiltinId(_, _)
            | I::CallBuiltinDiscardId(_, _)
            | I::CallLocal(_, _)
            | I::CallUser(_, _)
            | I::TailCallLocal(_, _)
            | I::TailCallUser(_, _)
            | I::CallAnon(_)
            | I::TailCallAnon(_)
            | I::Postfix(_)
            | I::TailPostfix(_)
    )
}

fn instruction_amount(op: &Instruction) -> usize {
    use Instruction as I;
    match op {
        I::Cat(count)
        | I::MakeList(count)
        | I::MakeDict(count)
        | I::IndexMany(count)
        | I::Postfix(count)
        | I::TailPostfix(count)
        | I::CallAnon(count)
        | I::TailCallAnon(count) => *count,
        I::CallBuiltinId(id, argc) | I::CallBuiltinDiscardId(id, argc) => {
            usize::from(*id) ^ usize::from(*argc)
        }
        I::CallLocal(slot, argc)
        | I::TailCallLocal(slot, argc)
        | I::IndexManyLoadLocal(slot, argc)
        | I::IndexManyLoadCapture(slot, argc)
        | I::IndexManyAssignLocal(slot, argc)
        | I::IndexManyAssignCapture(slot, argc)
        | I::IndexManyAssignLocalDrop(slot, argc)
        | I::IndexManyAssignCaptureDrop(slot, argc) => usize::from(*slot) ^ *argc,
        I::CallUser(name, argc) | I::TailCallUser(name, argc) => name.len() ^ *argc,
        I::IndexManyLoadVar(name, argc)
        | I::IndexManyAssignVar(name, argc)
        | I::IndexManyAssignVarDrop(name, argc) => name.len() ^ *argc,
        I::CmpChain(ops) => ops.len(),
        I::Jump(target)
        | I::JumpIfFalse(target)
        | I::JumpIfGE(target)
        | I::BoolAndLazy(target)
        | I::BoolOrLazy(target) => *target,
        I::JumpIfCmpFalse(data) => data.target,
        I::NLoopEnter(data) => {
            usize::from(data.index)
                ^ usize::from(data.count)
                ^ usize::from(data.snapshot)
                ^ data.target
        }
        I::NLoopNext(data) => usize::from(data.snapshot) ^ usize::from(data.index) ^ data.target,
        I::JumpIfLEZLocal(slot, target) => usize::from(*slot) ^ *target,
        I::JumpIfNamedProvided(slot, bit, target) => {
            usize::from(*slot) ^ usize::from(*bit) ^ *target
        }
        I::MakeRange {
            inclusive,
            has_next,
        } => usize::from(*inclusive) + (usize::from(*has_next) << 1),
        _ => 0,
    }
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn usize_to_u64_hash(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn hash_coord(seed: u64, len: usize) -> usize {
    let len = u64::try_from(len).expect("sampler dimension fits in u64");
    let coord = seed % len;
    usize::try_from(coord).expect("sampler coordinate fits in usize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::BinaryOperator;
    use crate::session::stdio::WqOutput;
    use crate::vm::inst::Operand;

    impl SampleInterpreter {
        fn with_mode(mode: RenderMode) -> Self {
            Self {
                art: RefCell::new(InstructionArt::new(mode, false)),
                io_error: RefCell::new(None),
                automatic: false,
                running: false,
            }
        }
    }

    #[test]
    fn sample_interpreter_delegates_to_vanilla() {
        let insts = vec![
            Instruction::load_const(Value::Int(1)),
            Instruction::load_const(Value::Int(2)),
            Instruction::binary_op(BinaryOperator::Add, Operand::Stack, Operand::Stack),
            Instruction::Return,
        ];
        let len = insts.len();
        let mut vm = Vm::new(insts);
        let mut interpreter = SampleInterpreter::with_mode(RenderMode::Off);

        let result = interpreter
            .interpret(&mut vm, len)
            .expect("sample should execute through vanilla");

        assert_eq!(result, Value::Int(3));
        assert_eq!(interpreter.art.borrow().ops, 4);
        assert_eq!(interpreter.art.borrow().last_label, "stack");
    }

    struct FailingOutput;

    impl WqOutput for FailingOutput {
        fn write(&mut self, _text: &str) -> Result<(), WqIoError> {
            Err(WqIoError::Other("closed sink".to_string()))
        }
    }

    #[test]
    fn sample_interpreter_propagates_session_output_errors() {
        let insts = vec![Instruction::load_const(Value::Int(1)), Instruction::Return];
        let len = insts.len();
        let mut vm = Vm::new(insts);
        vm.runtime_io.set_stderr(Box::new(FailingOutput));
        let mut interpreter = SampleInterpreter::with_mode(RenderMode::FinalOnly);

        let error = interpreter
            .interpret(&mut vm, len)
            .expect_err("sample renderer should propagate output failure");

        assert_eq!(error.err_type, WqErrorType::Io);
        assert!(error.to_string().contains("closed sink"));
    }

    #[test]
    fn automatic_render_policy_uses_the_session_stderr_capability() {
        assert_eq!(
            InstructionArt::configured(None, false, false).mode,
            RenderMode::FinalOnly
        );
        assert_eq!(
            InstructionArt::configured(None, true, true).mode,
            RenderMode::Animated
        );
        assert_eq!(
            InstructionArt::configured(Some("force"), false, false).mode,
            RenderMode::Animated
        );
    }

    #[test]
    fn binary_ops_paint_yellow_pixels() {
        let mut art = InstructionArt::new(RenderMode::FinalOnly, false);
        let vm = Vm::new(Vec::new());
        let op = Instruction::binary_op(BinaryOperator::Add, Operand::Stack, Operand::Stack);

        art.observe(&vm, 7, &op).expect("render sample");

        assert_eq!(art.ops, 1);
        assert_eq!(art.last_label, "op");
        assert!(
            art.pixels
                .iter()
                .flatten()
                .any(|pixel| { pixel.heat > 0 && pixel.color == DotColor::Yellow })
        );
    }

    #[test]
    fn frames_are_deterministic_for_same_instruction_stream() {
        let vm = Vm::new(Vec::new());
        let insts = [
            Instruction::load_const(Value::Int(1)),
            Instruction::load_const(Value::Int(2)),
            Instruction::binary_op(BinaryOperator::Add, Operand::Stack, Operand::Stack),
            Instruction::Return,
        ];
        let mut a = InstructionArt::new(RenderMode::FinalOnly, false);
        let mut b = InstructionArt::new(RenderMode::FinalOnly, false);

        for (idx, inst) in insts.iter().enumerate() {
            a.observe(&vm, idx, inst).expect("render first sample");
            b.observe(&vm, idx, inst).expect("render second sample");
        }

        assert_eq!(a.frame(), b.frame());
    }

    #[test]
    fn frame_uses_cat_star_chars_and_compact_legend() {
        let mut art = InstructionArt::new(RenderMode::FinalOnly, false);
        let vm = Vm::new(Vec::new());
        let op = Instruction::binary_op(BinaryOperator::Add, Operand::Stack, Operand::Stack);

        art.observe(&vm, 7, &op).expect("render sample");
        let frame = art.frame();

        assert!(!frame.contains("wq sample art"));
        assert!(!frame.contains("load store"));
        assert!(frame.ends_with("L S O C J B I K\n"));
        assert!(frame.chars().any(|ch| CAT_STAR_CHARS.contains(&ch)));
    }
}
