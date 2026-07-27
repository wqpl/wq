# Modules

`@i` evaluates one file in an isolated lexical scope and returns its final value.

```wq no-run
math:@i"math.wq"
math
```

The path must be an ordinary or raw string literal. These forms are valid:

```wq no-run
module:@i"module.wq"
windows_path:@i @l"lib\module.wq"
```

Computed paths and format strings are rejected. Use a separate `@i` expression for each statically known dependency.

## Export one value

A module exports its final expression. A useful pattern is a dict of public functions and constants:

```wq
// counter.wq
count:0
next:'{count+:1}
(`next:next;`initial:count)
```

The importing file receives only that dict:

```wq no-run
(`next;`initial):@i"counter.wq"
next[]
```

Top-level assignments such as `count` stay private. Exported functions retain private bindings through normal closure capture. An empty or comment-only module exports `()`.

## Resolution

Relative paths are resolved from the source file containing `@i`, not from the process working directory at the time the expression runs. Inline and REPL sources capture their working directory when they are compiled.

The CLI canonicalizes filesystem paths for stable module identity. It does not add a `.wq` extension. Other hosts provide their own resolver, and a host without one reports an `io` error.

## Execution and caching

`@i` is an ordinary primary expression, so it works in assignments, function bodies, and conditional branches:

```wq no-run
$[use_extra;@i"extra.wq";()]
```

Resolution and initialization happen only when execution reaches the expression. A successful module runs once per session workspace. Later imports with the same stable identity return the cached export value, including its captured state.

Failed imports are not cached. A later import retries resolution and initialization. Import cycles report the identity chain.

## Errors

`@t` catches resolver, file, syntax, compilation, initialization, and cycle errors:

```wq no-run
result:@t @i"optional.wq"
```

External effects that occurred before a module failed are not rolled back.

## `@i` and `\load`

`@i` is the module system. It isolates bindings and exports one value.

`\load` remains the legacy script inclusion mechanism. It evaluates code in the current workspace and can introduce or replace top-level bindings. Imported modules reject legacy loader directives.
