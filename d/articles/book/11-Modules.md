# Modules

Use modules to split a program into files without sharing every binding.
`@i"path.wq"` runs a file in its own scope and returns its final value.

The first example uses two files in the same directory.

## Export One Value

Create `counter.wq`. The final expression is a dict, so that dict is the value
returned by `@i`:

<!-- wq-example {"id":"counter-file","workspace":"counter","file":"counter.wq"} -->
```wq
count:0
next:'{count+:1}
(`next:next;`initial:count)
```

The leading `'` lets `next` update the captured `count` binding. That binding
belongs to `counter.wq` and is not visible to the importing file.

Create `main.wq` beside it. Import the dict and unpack the two entries:

<!-- wq-example {"id":"counter-entry","workspace":"counter","entry":true,"expect":{"value":"(0;1;2)"}} -->
```wq
(`next;`initial):@i"counter.wq"
(initial;next[];next[])
```

Run `wq main.wq -p` to produce `(0;1;2)`.

An empty module returns `()`. A module can return any one value, but a function
or dict makes a convenient public interface.

## Run Code Only From the Entry File

`main?[]` returns `T` in the entry script and `F` in an imported module. Use it
when a file should work both as a reusable module and as a command-line entry
point:

```wq
run:{[]echo "running directly"}
$.[main?[];run[]]
run
```

Here the final `run` exports the function. Running this file directly also calls
it through the guard. If another file imports it, the guard does nothing.

A function uses the `main?[]` status of the file where it was defined. An
exported function from a module therefore continues to receive `F` when called
by the entry script.

## Literal Paths

Write the import path directly after `@i` as an ordinary or raw string literal:

<!-- wq-example {"id":"module-path-forms","role":"syntax"} -->
```wq
module:@i"module.wq"
windows_path:@i @l"lib\module.wq"
```

Variables, formatted strings and other computed paths are not accepted. A
relative path starts from the file containing that `@i` expression, so imports
inside a module are relative to that module.

## When a Module Runs

An import runs only when execution reaches it. After a successful import, later
imports of the same module in that wq session return the same exported value.
Captured state is shared, so importing `counter.wq` twice does not create two
counters.

A failed import is not remembered. A later import tries again. If modules import
one another in a cycle, wq reports the chain of files in the cycle.

## Conditional Imports

Because `@i` is an expression, it can appear in a branch. Here `feature.wq` is
not run because `use_extra` is `F`:

<!-- wq-example {"id":"feature-file","workspace":"conditional-module","file":"feature.wq"} -->
```wq
(`enabled:T)
```

<!-- wq-example {"id":"conditional-entry","workspace":"conditional-module","entry":true,"expect":{"value":"()"}} -->
```wq
use_extra:F
$[use_extra;@i"feature.wq";()]
```

## Import Errors

`@t` catches import failures such as a missing file, invalid module code, an
error while the module runs or an import cycle:

```wq
result:@t @i"missing.wq"
result 0
```

The result begins with the `` `error `` tag. Work completed before the failure,
such as output already printed by the module, is not undone.

## `@i` and `\load`

Use `@i` when one file depends on another. It keeps the imported file's bindings
private and returns one export value.

`\load` instead evaluates a file in the current session. Its top-level bindings
become part of that session and can replace existing bindings. It does not
create a module scope.

## Summary

- `@i"path.wq"` evaluates an isolated module and returns its final value.
- Export a function or dict to provide a convenient public interface.
- A successfully imported module runs once per wq session.
- Relative paths resolve from the source file containing the import.
- `main?[]` distinguishes entry-script code from imported module code.
- Continue to **A Prime Sieve** to combine the core language in one program.
