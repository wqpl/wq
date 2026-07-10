# Errors

wq errors are meant to point at the expression that broke.

```wq error
1/0
```

The message should tell you what happened and where the bad value came from.

## Raise Your Own

Use `raise` when a condition makes the rest of the computation meaningless.

```wq error
raise "nope"
```

Small functions often use this for guard rails.

```wq error
head:{[xs]$.[#xs=0;raise "empty list"];xs 0}
head ()
```

## Assertions

`@a expr` asserts that an expression is true.

```wq
@a 2<3
"still here"|echo
```

If the assertion is false, execution stops.

```wq error
@a 2>3
```

## Try

`@t expr` preserves either the successful value or the error as a tagged pair.

```wq
(@t 1+1)|echo
(@t 1/0)|echo
(@t raise "boom")|echo
```

Success has the shape ``(`ok;value)``. Failure has the shape
``(`error;error_dict)``. Check the first item before reading the payload.

```wq
result:@t 1+1
$[result 0=`ok;result 1;raise "unexpected failure"]
```

The error dict has stable `version`, `kind`, `message`, `source`, `span`,
`notes`, `data`, `stack`, and `cause` fields. `message` and `notes` are for
people. Branch on the `kind` tag instead of parsing those strings.

## Keep

- Errors stop the current run.
- `raise` creates an error intentionally.
- `@a` asserts a truth.
- `@t` catches failure and returns a tagged result with the value or error.
