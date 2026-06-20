use std::cell::RefCell;
use std::fmt::Write as _;
use std::io::Write as _;

use crate::interpret::vanilla::VanillaInterpreter;
use crate::interpret::{Interpreter, InterpreterHook, InterpreterKind};
use crate::value::{Value, WqResult};
use crate::vm::Vm;
use crate::vm::inst::Instruction;

const WIDTH: usize = 48;
const HEIGHT: usize = 14;
const FRAME_INTERVAL: usize = 12;
const DECAY: u8 = 18;
const CAT_STAR_CHARS: [char; 3] = ['*', '•', '+'];

pub(crate) struct SampleInterpreter {
    art: RefCell<InstructionArt>,
}

impl Default for SampleInterpreter {
    fn default() -> Self {
        Self {
            art: RefCell::new(InstructionArt::auto()),
        }
    }
}

impl Interpreter for SampleInterpreter {
    fn interpret(&mut self, vm: &mut Vm, limit: usize) -> WqResult<Value> {
        let mut delegate = VanillaInterpreter;
        let previous_interpreter = vm.interpreter_kind;
        vm.interpreter_kind = InterpreterKind::Vanilla;
        vm.set_hooks(Some(self));
        let result = delegate.interpret(vm, limit);
        vm.set_hooks(None);
        vm.interpreter_kind = previous_interpreter;
        self.art.borrow_mut().finish();
        result
    }
}

impl InterpreterHook for SampleInterpreter {
    fn before_instruction(&self, vm: &Vm, idx: usize, op: &Instruction) {
        self.art.borrow_mut().observe(vm, idx, op);
    }
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
    fn auto() -> Self {
        let setting = std::env::var("WQ_SAMPLE_ART").ok();
        let force = matches!(setting.as_deref(), Some("1" | "on" | "force" | "animate"));
        let mode = match setting.as_deref() {
            Some("0" | "off" | "false" | "quiet") => RenderMode::Off,
            Some("static" | "final") => RenderMode::FinalOnly,
            Some("1" | "on" | "force" | "animate") => RenderMode::Animated,
            _ if stderr_is_terminal() => RenderMode::Animated,
            _ => RenderMode::FinalOnly,
        };
        let color = std::env::var_os("NO_COLOR").is_none() && (force || stderr_is_terminal());
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

    fn observe(&mut self, vm: &Vm, pc: usize, op: &Instruction) {
        self.ops += 1;
        self.max_stack_len = self.max_stack_len.max(vm.stack.len());
        self.max_call_depth = self.max_call_depth.max(vm.locals.len());
        self.last_pc = pc;

        let signal = signal_for(op);
        self.last_label = signal.label;
        if self.mode == RenderMode::Off {
            return;
        }

        self.fade();

        let seed = mix(signal.salt
            ^ (pc as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ (self.ops as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9)
            ^ ((vm.stack.len() as u64) << 32)
            ^ ((vm.locals.len() as u64) << 48));
        let x = (seed as usize) % WIDTH;
        let y = ((seed >> 16) as usize) % HEIGHT;
        self.paint(x, y, signal);

        if self.mode == RenderMode::Animated
            && (self.ops == 1 || self.ops.is_multiple_of(FRAME_INTERVAL))
        {
            self.render();
        }
    }

    fn finish(&mut self) {
        if self.mode == RenderMode::Off || self.ops == 0 {
            return;
        }
        self.render();
        if self.started {
            let mut stderr = std::io::stderr().lock();
            let _ = stderr.write_all(b"\x1b[?25h\n");
            let _ = stderr.flush();
        }
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

                let falloff = (distance as u8).saturating_mul(42);
                let heat = signal.strength.saturating_sub(falloff);
                if heat == 0 {
                    continue;
                }

                let pixel = &mut self.pixels[ny as usize][nx as usize];
                pixel.heat = pixel.heat.saturating_add(heat);
                pixel.color = signal.color;
            }
        }
    }

    fn render(&mut self) {
        let frame = self.frame();
        let line_count = HEIGHT + 1;
        let mut stderr = std::io::stderr().lock();

        if self.mode == RenderMode::Animated {
            if self.started {
                let _ = write!(stderr, "\x1b[{line_count}F");
            } else {
                let _ = stderr.write_all(b"\x1b[?25l");
                self.started = true;
            }
        }

        let _ = stderr.write_all(frame.as_bytes());
        let _ = stderr.flush();
        self.frames += 1;
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
            salt: 0x5a17_5a17 ^ instruction_amount(op) as u64,
        }
    } else if is_jump(op) {
        Signal {
            label: "jump",
            color: DotColor::White,
            strength: 185,
            radius: 1,
            salt: 0x70ad_70ad ^ instruction_amount(op) as u64,
        }
    } else if is_build(op) {
        Signal {
            label: "build",
            color: DotColor::Red,
            strength: 230,
            radius: 2,
            salt: 0xb11d_b11d ^ instruction_amount(op) as u64,
        }
    } else if is_op(op) {
        Signal {
            label: "op",
            color: DotColor::Yellow,
            strength: 205,
            radius: 1,
            salt: 0x0f0f_0f0f ^ instruction_amount(op) as u64,
        }
    } else if is_store(op) {
        Signal {
            label: "store",
            color: DotColor::Green,
            strength: 190,
            radius: 1,
            salt: 0x570e_570e ^ instruction_amount(op) as u64,
        }
    } else if is_index(op) {
        Signal {
            label: "index",
            color: DotColor::Cyan,
            strength: 175,
            radius: 1,
            salt: 0x1d3c_1d3c ^ instruction_amount(op) as u64,
        }
    } else if is_load(op) {
        Signal {
            label: "load",
            color: DotColor::Cyan,
            strength: 160,
            radius: 0,
            salt: 0x10ad_10ad ^ instruction_amount(op) as u64,
        }
    } else {
        Signal {
            label: "stack",
            color: DotColor::White,
            strength: 140,
            radius: 0,
            salt: 0x57ac_57ac ^ instruction_amount(op) as u64,
        }
    }
}

fn is_load(op: &Instruction) -> bool {
    use Instruction as I;
    matches!(
        op,
        I::LoadConst(_)
            | I::LoadClosure(_)
            | I::LoadVar(_)
            | I::LoadVarExists(_)
            | I::LoadCapture(_)
            | I::LoadSelf
            | I::LoadLocal(_)
            | I::LoadNamedArgsProvided(_)
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
            | I::StoreCaptureKeep(_)
            | I::IndexAssignVar(_)
            | I::IndexAssignLocal(_)
            | I::IndexAssignCapture(_)
            | I::IndexAssignVarDrop(_)
            | I::IndexAssignLocalDrop(_)
            | I::IndexAssignCaptureDrop(_)
            | I::IndexMutate { .. }
    )
}

fn is_op(op: &Instruction) -> bool {
    use Instruction as I;
    matches!(op, I::BinaryOp(_) | I::UnaryOp(_) | I::CmpChain(_))
}

fn is_jump(op: &Instruction) -> bool {
    use Instruction as I;
    matches!(
        op,
        I::Jump(_)
            | I::JumpIfFalse(_)
            | I::JumpIfCmpFalse(_)
            | I::JumpIfGE(_)
            | I::JumpIfLEZLocal(_, _)
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
        I::Index | I::IndexLoadLocal(_) | I::IndexLoadCapture(_) | I::IndexLoadVar(_)
    )
}

fn is_call(op: &Instruction) -> bool {
    use Instruction as I;
    matches!(
        op,
        I::CallBuiltinId(_, _)
            | I::CallLocal(_, _)
            | I::CallUser(_, _)
            | I::TailCallLocal(_, _)
            | I::TailCallUser(_, _)
            | I::CallAnon(_)
            | I::TailCallAnon(_)
            | I::Postfix(_)
            | I::TailPostfix(_)
            | I::PostfixLocal(_, _)
            | I::TailPostfixLocal(_, _)
            | I::PostfixMethodLocal(_, _, _)
            | I::TailPostfixMethodLocal(_, _, _)
            | I::CallMethodLocal(_, _, _)
            | I::TailCallMethodLocal(_, _, _)
            | I::PostfixCapture(_, _)
            | I::TailPostfixCapture(_, _)
            | I::PostfixMethodCapture(_, _, _)
            | I::TailPostfixMethodCapture(_, _, _)
            | I::CallMethodCapture(_, _, _)
            | I::TailCallMethodCapture(_, _, _)
            | I::PostfixVar(_, _)
            | I::TailPostfixVar(_, _)
            | I::PostfixMethodVar(_, _, _)
            | I::TailPostfixMethodVar(_, _, _)
            | I::CallMethodVar(_, _, _)
            | I::TailCallMethodVar(_, _, _)
    )
}

fn instruction_amount(op: &Instruction) -> usize {
    use Instruction as I;
    match op {
        I::Cat(count)
        | I::MakeList(count)
        | I::MakeDict(count)
        | I::Postfix(count)
        | I::TailPostfix(count)
        | I::CallAnon(count)
        | I::TailCallAnon(count) => *count,
        I::CallBuiltinId(id, argc) => usize::from(*id) ^ usize::from(*argc),
        I::CallLocal(slot, argc)
        | I::TailCallLocal(slot, argc)
        | I::PostfixLocal(slot, argc)
        | I::TailPostfixLocal(slot, argc)
        | I::PostfixCapture(slot, argc)
        | I::TailPostfixCapture(slot, argc) => usize::from(*slot) ^ *argc,
        I::CallUser(name, argc) | I::TailCallUser(name, argc) => name.len() ^ *argc,
        I::PostfixMethodLocal(slot, name, argc)
        | I::TailPostfixMethodLocal(slot, name, argc)
        | I::CallMethodLocal(slot, name, argc)
        | I::TailCallMethodLocal(slot, name, argc)
        | I::PostfixMethodCapture(slot, name, argc)
        | I::TailPostfixMethodCapture(slot, name, argc)
        | I::CallMethodCapture(slot, name, argc)
        | I::TailCallMethodCapture(slot, name, argc) => usize::from(*slot) ^ name.len() ^ *argc,
        I::PostfixVar(name, argc) | I::TailPostfixVar(name, argc) => name.len() ^ *argc,
        I::PostfixMethodVar(receiver, name, argc)
        | I::TailPostfixMethodVar(receiver, name, argc)
        | I::CallMethodVar(receiver, name, argc)
        | I::TailCallMethodVar(receiver, name, argc) => receiver.len() ^ name.len() ^ *argc,
        I::CmpChain(ops) => ops.len(),
        I::Jump(target)
        | I::JumpIfFalse(target)
        | I::JumpIfGE(target)
        | I::BoolAndLazy(target)
        | I::BoolOrLazy(target) => *target,
        I::JumpIfCmpFalse(data) => data.target,
        I::JumpIfLEZLocal(slot, target) => usize::from(*slot) ^ *target,
        I::MakeRange {
            inclusive,
            has_step,
        } => usize::from(*inclusive) + (usize::from(*has_step) << 1),
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

#[cfg(not(target_arch = "wasm32"))]
fn stderr_is_terminal() -> bool {
    use std::io::IsTerminal as _;

    std::io::stderr().is_terminal()
}

#[cfg(target_arch = "wasm32")]
fn stderr_is_terminal() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::astnode::BinaryOperator;
    use crate::vm::inst::Operand;

    impl SampleInterpreter {
        fn with_mode(mode: RenderMode) -> Self {
            Self {
                art: RefCell::new(InstructionArt::new(mode, false)),
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

    #[test]
    fn binary_ops_paint_yellow_pixels() {
        let mut art = InstructionArt::new(RenderMode::FinalOnly, false);
        let vm = Vm::new(Vec::new());
        let op = Instruction::binary_op(BinaryOperator::Add, Operand::Stack, Operand::Stack);

        art.observe(&vm, 7, &op);

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
            a.observe(&vm, idx, inst);
            b.observe(&vm, idx, inst);
        }

        assert_eq!(a.frame(), b.frame());
    }

    #[test]
    fn frame_uses_cat_star_chars_and_compact_legend() {
        let mut art = InstructionArt::new(RenderMode::FinalOnly, false);
        let vm = Vm::new(Vec::new());
        let op = Instruction::binary_op(BinaryOperator::Add, Operand::Stack, Operand::Stack);

        art.observe(&vm, 7, &op);
        let frame = art.frame();

        assert!(!frame.contains("wq sample art"));
        assert!(!frame.contains("load store"));
        assert!(frame.ends_with("L S O C J B I K\n"));
        assert!(frame.chars().any(|ch| CAT_STAR_CHARS.contains(&ch)));
    }
}
