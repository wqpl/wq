# Modules

A module is a wq file with a private lexical scope. `@i` evaluates that file
once and returns its final value.

The examples below form one workspace. wqide registers each named file for the
entry block. With the native CLI, save the named block beside an entry script,
then run that entry script.

## Export One Value

Create `counter.wq`. Its final dict is the module export:

<!-- wq-example {"id":"counter-file","workspace":"counter","file":"counter.wq"} -->
```wq
count:0
next:'{count+:1}
(`next:next;`initial:count)
```

The `'` before the function literal captures `count` by reference, so exported
`next` can update the module's private state.

Import the module and unpack its public entries. The expected result is
`(0;1;2)`.

<!-- wq-example {"id":"counter-entry","workspace":"counter","entry":true,"expect":{"value":"(0;1;2)"}} -->
```wq
(`next;`initial):@i"counter.wq"
(initial;next[];next[])
```

In the CLI, save that second block as `main.wq` beside `counter.wq`, then run
`wq main.wq -p`.

Only the exported dict reaches the importing code. The top-level `count`
binding remains private.

## Literal Paths

Import paths must be ordinary or raw string literals. These are syntax
illustrations rather than one shared workspace:

<!-- wq-example {"id":"module-path-forms","role":"syntax"} -->
```wq
module:@i"module.wq"
windows_path:@i @l"lib\module.wq"
```

Computed paths and format strings are rejected. Use a separate `@i` expression
for every statically known dependency.

## Resolution and Caching

| Question | Contract |
| --- | --- |
| Where does a relative path start? | At the source file containing `@i` |
| When does the module run? | When execution first reaches the import |
| How often does it run? | Once per session workspace after a successful import |
| What does a later import return? | The cached export value and captured state |
| Is a failed import cached? | No; a later import retries |
| What happens in a cycle? | The error reports the module identity chain |

The CLI canonicalizes filesystem paths for stable module identity. Other hosts
provide their own resolver. wqide uses exact virtual module names for examples.

## Conditional Imports

`@i` is an expression, so it can appear in a branch. This workspace registers
`feature.wq`, but the false branch avoids importing it. The expected result is
`()`.

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

The expected output is `` `error ``. External effects that occurred before a
module failed are not rolled back.

## `@i` and `\load`

`@i` is the module system. It isolates bindings and exports one value.

`\load` is the legacy script-inclusion mechanism. It evaluates code in the
current workspace and can introduce or replace top-level bindings. Imported
modules reject legacy loader directives.

## Keep

- `@i"path.wq"` evaluates an isolated module and returns its final value.
- Export a function or dict instead of exposing private bindings.
- Successful imports are cached per session workspace.
- Relative paths resolve from the source file containing the import.
- Continue to **A Prime Sieve** to combine the core language in one program.
