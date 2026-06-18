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
(diff expr)|echo
```

Use parentheses when piping around symbolic expressions. Otherwise the pipe can become part of what `@s` tries to quote.

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
```

The roots are values now, so list arithmetic works.

## Keep

- `@s expr` creates a symbolic expression.
- After `@s`, pass the symbolic value to CAS functions.
- Prefer `factor[@s ...]` or parentheses when pipes are nearby.
- Ordinary arithmetic and CAS arithmetic are related, but not the same mode.
