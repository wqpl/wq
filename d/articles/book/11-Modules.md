# Modules

A module is a wq file with a private lexical scope. `@i` returns its final value
as the export.

The examples below form one workspace. wqide registers named files for the
entry block. In the CLI, save them beside the entry script.

## Export One Value

Create `counter.wq`. Its final dict is the module export:

<!-- wq-example {"id":"counter-file","workspace":"counter","file":"counter.wq"} -->
```wq
count:0
next:'{count+:1}
(`next:next;`initial:count)
```

The `'` captures `count` by reference, allowing exported `next` to update the
module's private state.

Import the module and unpack its public entries. The expected result is
`(0;1;2)`.

<!-- wq-example {"id":"counter-entry","workspace":"counter","entry":true,"expect":{"value":"(0;1;2)"}} -->
```wq
(`next;`initial):@i"counter.wq"
(initial;next[];next[])
```

In the CLI, save that second block as `main.wq` beside `counter.wq`, then run
`wq main.wq -p`.

The importing code receives the exported dict. The top-level `count` binding
remains private.

## Entry-Only Behavior

`main?[]` returns `T` in the entry script and `F` in an imported module. Use it
to guard a command-line entry point:

```wq
run:{[]echo "running directly"}
$.[main?[];run[]]
run
```

Functions retain the status of their defining file. An exported module function
continues to observe `F`.

## Literal Paths

Import paths must be ordinary or raw string literals. These are syntax
illustrations rather than one shared workspace:

<!-- wq-example {"id":"module-path-forms","role":"syntax"} -->
```wq
module:@i"module.wq"
windows_path:@i @l"lib\module.wq"
```

Each statically known dependency uses its own `@i` expression.

## Resolution and Caching

| Question | Contract |
| --- | --- |
| Where does a relative path start? | At the source file containing `@i` |
| When does the module run? | When execution first reaches the import |
| Execution count | Once per session workspace after a successful import |
| What does a later import return? | The cached export value and captured state |
| Failed import | A later import retries |
| What happens in a cycle? | The error reports the module identity chain |

The CLI canonicalizes filesystem paths for stable module identity. Other hosts
provide their own resolver. wqide uses exact virtual module names for examples.

## Conditional Imports

`@i` is an expression, so it can appear in a branch. This false branch returns
`()`, leaving `feature.wq` unevaluated.

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

`@t` catches resolver, file, syntax, compilation, initialization and cycle
errors. This example succeeds by returning a tagged error value:

```wq
result:@t @i"missing.wq"
result 0
```

The expected output is `` `error ``. External effects completed before the
failure remain.

## `@i` and `\load`

`@i` isolates bindings and exports one value. `\load` evaluates a legacy script
in the current workspace and can introduce or replace top-level bindings.

## Summary

- `@i"path.wq"` evaluates an isolated module and returns its final value.
- Export a function or dict instead of exposing private bindings.
- Successful imports are cached per session workspace.
- Relative paths resolve from the source file containing the import.
- `main?[]` distinguishes entry-script code from imported module code.
- Continue to **A Prime Sieve** to combine the core language in one program.
