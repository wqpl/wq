# CAS

wq can carry symbolic math as data. CAS values start with `@s`.

```wq
expr:@s x^2+2*x+1
expr|echo
```

This is different from ordinary evaluation. A bare `x^2+2*x+1` tries to use a bound value named `x`; `@s` quotes the expression for symbolic work.

## Transform The Expression

Once you have a symbolic value, apply CAS functions to that value.

```wq
expr:@s x^2+2*x+1
expr|diff|echo
expr|substitute[`x:2]|echo
expr[`x:2]|echo
```

`@s` quotes the symbolic expression, then the value can move through normal pipes. Named arguments on CAS calls, and on CAS values themselves, bind symbolic variables.

## Factor And Solve

```wq
factor[@s x^2-1]|echo
solve[@s x^2=1]|echo
```

The bracket calls keep the symbolic expression neatly inside the argument list.

## Numeric And Symbolic Can Meet

CAS output can be moved through normal wq code.

```wq
roots:solve[@s x^2=1]
roots+10|echo

f:@s sin[x]+y
f|numeric[`x:0;`y:2]|echo
```

The roots are values now, so list arithmetic works.
The expression `f` is a value too; `numeric` can bind `x` and `y` before evaluating it.

## Keep

- `@s expr` creates a symbolic expression.
- After `@s`, pass the symbolic value to CAS functions.
- Pipes stay outside the symbolic quote, so `@s x^2|diff` works as ordinary value flow.
- Ordinary arithmetic and CAS arithmetic are related, but not the same mode.
