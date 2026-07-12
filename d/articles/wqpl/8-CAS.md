# CAS

wq can carry symbolic math as data. CAS values start with `@s`.

```wq
expr:@s x^2+2*x+1
expr|echo
```

This is different from ordinary evaluation.
A bare `x^2+2*x+1` tries to use a bound value named `x`; `@s` quotes the expression for symbolic work.

## Transform The Expression

Once you have a symbolic value, apply CAS functions to that value.

```wq
expr:@s x^2+2*x+1
expr|diff|echo
expr|I|echo
integrate[@s 1/(x+a);@s x]|echo
expr|substitute[`x:2]|echo
expr[`x:2]|echo
expr[2]|echo
```

`@s` quotes the symbolic expression, then the value can move through normal pipes.
Named arguments on CAS calls, and on CAS values themselves, bind symbolic variables.
A single-variable CAS expression can also be called with one positional argument.

## Factor And Solve

```wq
factor[@s x^2-1]|echo
solve[@s x^2=1]|echo
solve[@s a*x;@s x]|echo
solve[@s x^2+1;`domain:`real]|echo
solve_system[@s(2*x+y=5;x-y=1)]|echo
solve_system[(eq[@s 2*x+y;@s b];eq[@s x-y;@s c]);(@s x;@s y)]|echo
```

The bracket calls keep the symbolic expression neatly inside the argument list.
For linear systems, `solve_system` can infer variables and returns a dict keyed by variable name.
Passing solve variables explicitly controls dict order, and other symbols can remain as parameters in supported linear and quadratic solves.

When a symbolic coefficient can change a polynomial's degree or a system's rank, the solver returns explicit cases.
Each case has a `when` list of conditions and a branch result.
You can narrow those cases with assumptions:

```wq
solve_system[
  (eq[@s a*x+b*y;1];eq[@s c*x+d*y;2]);
  (@s x;@s y);
  `assuming:nonzero[@s a*d-b*c]]|echo
```

Use `nonzero[expr]` for a nonzero condition and `eq[expr;0]` for a zero condition. A list passed to named argument `assuming` represents their conjunction.

Conditions can also describe `zero`, `positive`, `negative`, `nonnegative`, `real`, and `integer` expressions. The CAS derives basic consequences such as positive values being nonzero and integers being real. Contradictory assumptions are rejected.

For `solve`, a finite root set is a list, the `all` tag means every value is a solution, and an empty list means there is no solution. The default domain is the `complex` tag. Pass named argument `domain` with the `real` tag to exclude non-real roots. A parameterized real-domain solve also needs `real` assumptions for its symbolic coefficients, so the solver does not silently assume that a parameter is real. For `solve_system`, a unique solution stays a variable dict, the `none` tag means no solution, and dependent systems return a `solution` dict with a `parameters` list of fresh symbols.

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
