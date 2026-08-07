# Values and Display

wq programs move values through operators and functions. Non-container values
are atoms. Lists and dicts are containers.

## Common Categories

| Category | Example | What it represents |
| --- | --- | --- |
| `int` | `42` | An exact whole number |
| `float` | `3.5` | A floating-point number |
| `fraction` | `7/.2` | An exact rational number |
| `bool` | `T` or `F` | A condition result |
| `char` | `"a"` | One Unicode scalar |
| `tag` | `` `name `` | A symbolic name |
| `list` | `(1;2;3)` | An ordered container |
| `dict` | ``(`name:"Ada";`age:3)`` | An ordered keyed container |

`type[value]` reports the public category:

```wq
(type[42];type[3.5];type[T];type[(1;2;3)])
```

The expected output is `("int";"float";"bool";"list")`.

## Chars and Strings

A quoted literal that decodes to one Unicode scalar is a char. Longer quoted
literals are strings.

```wq
(type["a"];type["Ada"])
```

The expected output is `("char";"list")`. Strings belong to the public `list`
category.

Use leading comma for a one-character string:

```wq
("a";,"a";type["a"];type[,"a"])
```

The char displays as `"a"`. The one-character string displays as `,"a"`.

`""` is an empty string in source. Both an empty string and an empty list
display as `()`. Use `type` or the surrounding operation to distinguish them.

## Bools Are Exact

Comparisons produce `T` or `F`:

```wq
(2<3;2=3)
```

Branches and loops accept bool conditions only.

<!-- wq-example {"id":"bool-condition","expect":{"error":"domain"}} -->
```wq
$[1;"yes";"no"]
```

The expected diagnostic says `expected bool` and identifies `1` as an int.

## Tags

Tags begin with a backtick. They commonly name dict keys and named arguments:

```wq
(`ready;`name;`x2)
```

Tags are symbolic values. Identifier expressions perform binding lookup.

## Summary

- Atoms are non-container values. Lists and dicts are containers.
- A one-scalar quoted literal is a char. Longer quoted literals are strings.
- `T` and `F` are bools. Control flow accepts bool conditions only.
- `type[value]` reports the public category.
