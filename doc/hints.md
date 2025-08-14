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
