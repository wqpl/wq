# Debugging with wqdb

wqdb pauses a running program so you can see its current source expression,
call stack and bindings. You can then move through the program one step at a
time instead of adding temporary output calls.

## Try the Book Debugger

The Book REPL below keeps one session for all the expressions you run in it.
Its **Debug** control starts on for this exercise.

<!-- wq-example {"id":"wqdb-book-repl","repl":{"wqdb":true},"expect":{"value":"11"}} -->
```wq
adjust:{[n]
  doubled:n*2
  result:doubled+1
  @p result
  result
}
adjust 5
```

Choose **Run**. Execution pauses before the first expression and attaches a
debugger beside the transcript. The source view marks the current line. The
stack and binding sections explain how execution reached it and which values
are available.

Use **Continue** when you only care about the deliberate `@p` pause. At that
pause, `n`, `doubled` and `result` are visible in **Locals**. Continue again to
finish and return `11` to the transcript.

The session remains active after that run. Enter another expression to reuse
its bindings, turn **Debug** off to skip the automatic entry pause or choose
**Reset** to restore the original exercise. A deliberate `@p` still pauses
execution when **Debug** is off.

## Move Through Execution

wqdb offers four ways to leave a pause:

| wqide control | CLI command | What it does |
| --- | --- | --- |
| **Continue** | `c` | Run until a breakpoint, `@p`, error or completion |
| **Step over** | `n` | Run the current expression without entering a called function |
| **Step in** | `s` | Enter a function called by the current expression |
| **Step out** | `fin` | Finish the current function and return to its caller |

For the exercise above, step over until the active expression is `adjust 5`,
then step in. The selected stack frame changes to `adjust`, and its parameters
and local bindings become available for inspection.

## Read a Pause

Start with these debugger sections:

- **Source** shows the active expression and lets you toggle line breakpoints.
- **Stack** lists active function calls. Select a frame to inspect it.
- **Locals** shows parameters and bindings owned by the selected frame.
- **Globals** shows top-level bindings in the current session.
- **Instruction** shows the VM operation for the current stop.

The displayed field is called `kind` because debugger inspection includes
representation details such as `int-list` and `closure`. Normal language
results use public value categories instead.

## Pause at a Chosen Place

`@p` marks a useful source location for debugging. It accepts an optional
expression, pauses after that expression is evaluated and returns its value.
In the exercise, `@p result` pauses after `result` has been assigned.

In wqide, click the breakpoint marker beside a source line while paused, then
continue. A breakpoint stops before execution proceeds through that line.

Use **Symbol tracking** when the important event is a binding change. Enter a
visible global or local name and choose **Track**. Later writes to that binding
appear in **Changes**.

## Choose Step Size

The **Step by** control changes how much source one step covers:

- **Line** stops once per source line.
- **Expression** stops at each expression and is the default.
- **Instruction** stops before each VM instruction.

Expression stepping is the clearest starting point. Instruction stepping is
useful when investigating compilation or stack behavior.

## Use wqdb in the Terminal

Pass `-w` to begin a script under wqdb:

```sh
wq -w script.wq
```

The first pause opens a `wqdb[expr:N]` prompt. The stepping commands from the
table work there. These inspection commands cover most first investigations:

| Command | Result |
| --- | --- |
| `p` | Show source around the current stop |
| `lb` | Show local bindings |
| `gb` | Show global bindings |
| `bt` | Show the call stack |
| `g` | Show the current stepping granularity |
| `g line`, `g expr`, `g inst` | Change stepping granularity |
| `h` | Show all wqdb commands |

Inside the normal wq REPL, `\wqdb` toggles entry pauses for later evaluations
and `\wqdb.` enables them for the next evaluation only.

## Summary

- wqdb pauses execution with source, stack and binding information.
- Continue runs to the next stop. Step over, in and out control function calls.
- `@p` adds a deliberate source pause.
- Line breakpoints and symbol tracking stop at useful events.
- Expression stepping is the default. Line and instruction modes change the
  amount of work covered by each step.
- Continue to **Modules** to organize code across isolated files.
