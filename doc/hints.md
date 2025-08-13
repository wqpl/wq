## Functions

### Call

The following is invalid as a call:

```
fn -1  // This is parsed as BinaryOp{fn,-,1}
```

### Param
Functions defined without a param list can be called with 0, 1, 2 or 3 args, which are implicitly bound to x, y and z.

`fn:{[]...}` explicitly defines a function that accepts 0 args.

### Closure

Closures capture variables by value.

## Newline

newlines are generally accepted and can act as semicolons in most contexts.

```
mat:((1;2;3);(4;5;6);
     (7;8;9);(0;1;2))
fn:{echo x
    echo y}
```

### Important position

Some positions are considered "important" and a semicolon is explicitly required.

```
$[true;  // <- important
  1;     // <- important
  2]
N[3;     // <- important
  echo n]
```
