# Symbolic Math with CAS

This optional chapter introduces wq's computer algebra system (CAS). It is not
required for the rest of the introductory path.

wq can carry symbolic math as data. CAS values start with `@s`.

```wq
expr:@s x^2+2*x+1
expr
```

This is different from ordinary evaluation.
A bare `x^2+2*x+1` tries to use a bound value named `x`; `@s` quotes the expression for symbolic work.

## Transform The Expression

Once you have a symbolic value, apply CAS functions to that value.

<!-- wq-example {"id":"cas-differentiate","cellGroup":"cas-transforms"} -->
```wq
expr:@s x^2+2*x+1
expr|diff
```

<!-- wq-example {"id":"cas-integrate","cellGroup":"cas-transforms"} -->
```wq
expr|integrate
```

<!-- wq-example {"id":"cas-integrate-variable","cellGroup":"cas-transforms"} -->
```wq
integrate[@s 1/(x+a);@s x]
```

<!-- wq-example {"id":"cas-substitute","cellGroup":"cas-transforms"} -->
```wq
expr|substitute[`x:2]
```

<!-- wq-example {"id":"cas-call-named","cellGroup":"cas-transforms"} -->
```wq
expr[`x:2]
```

<!-- wq-example {"id":"cas-call-positional","cellGroup":"cas-transforms"} -->
```wq
expr[2]
```

`@s` quotes the symbolic expression, then the value can move through normal pipes.
Named arguments on CAS calls, and on CAS values themselves, bind symbolic variables.
A single-variable CAS expression can also be called with one positional argument.

## Factor And Solve

<!-- wq-example {"id":"cas-factor","cellGroup":"cas-solving"} -->
```wq
factor[@s x^2-1]
```

<!-- wq-example {"id":"cas-solve","cellGroup":"cas-solving"} -->
```wq
solve[@s x^2=1]
```

<!-- wq-example {"id":"cas-solve-variable","cellGroup":"cas-solving"} -->
```wq
solve[@s a*x;@s x]
```

<!-- wq-example {"id":"cas-solve-real","cellGroup":"cas-solving"} -->
```wq
solve[@s x^2+1;`domain:`real]
```

<!-- wq-example {"id":"cas-solve-system","cellGroup":"cas-solving"} -->
```wq
solve_system[@s(2*x+y=5;x-y=1)]
```

<!-- wq-example {"id":"cas-solve-system-variables","cellGroup":"cas-solving"} -->
```wq
solve_system[(eq[@s 2*x+y;@s b];eq[@s x-y;@s c]);(@s x;@s y)]
```

The bracket calls keep the symbolic expression inside the argument list.

| Call shape | Result |
| --- | --- |
| `solve[expression]` | Roots for one inferred variable |
| `solve[expression;variable]` | Roots for the selected variable |
| `solve_system[equations]` | A dict keyed by inferred variable names |
| `solve_system[equations;variables]` | A dict in the requested variable order |

Other symbols can remain as parameters in supported linear and quadratic
solves.

When a symbolic coefficient can change a polynomial's degree or a system's rank, the solver returns explicit cases.
Each case has a `when` list of conditions and a branch result.
You can narrow those cases with assumptions:

```wq
solve_system[
  (eq[@s a*x+b*y;1];eq[@s c*x+d*y;2]);
  (@s x;@s y);
  `assuming:@s nonzero[a*d-b*c]]
```

Use `@s nonzero[expr]` for a nonzero condition and `eq[expr;0]` for a zero
condition. The condition constructs `zero`, `nonzero`, `positive`, `negative`,
`nonnegative`, `real`, and `integer` exist only inside `@s`, so their names
remain available for ordinary wq bindings. A list passed to the named argument
`assuming` means all its conditions must hold.

The CAS derives basic consequences such as positive values being nonzero and integers being real. Contradictory assumptions are rejected.

Read solver results as follows:

- For `solve`, a finite root set is a list, the `` `all `` tag means every
  value is a solution, and an empty list means there is no solution.
- The default domain is the `` `complex `` tag. Pass the named argument
  `domain` with the `` `real `` tag to exclude non-real roots.
- A parameterized real-domain solve also needs `real` assumptions for its
  symbolic coefficients. The solver does not silently assume that a parameter
  is real.
- For `solve_system`, a unique solution is a variable dict, the `` `none `` tag
  means no solution, and dependent systems return a `` `solution `` dict with a
  `parameters` list of fresh symbols.

## Numeric And Symbolic Can Meet

CAS output can be moved through normal wq code.

<!-- wq-example {"id":"cas-numeric-roots","cellGroup":"cas-numeric"} -->
```wq
roots:solve[@s x^2=1]
roots+10
```

<!-- wq-example {"id":"cas-numeric-expression","cellGroup":"cas-numeric"} -->
```wq
f:@s sin[x]+y
f|numeric[`x:0;`y:2]
```

The roots are values now, so list arithmetic works.
The expression `f` is a value too; `numeric` can bind `x` and `y` before evaluating it.
Exact quoted function calls remain symbolic unless an exact identity applies. `numeric` is the explicit boundary that turns exact constants and functions into approximations.

## Keep

- `@s expr` creates a symbolic expression.
- After `@s`, pass the symbolic value to CAS functions.
- Pipes stay outside the symbolic quote, so `@s x^2|diff` works as ordinary value flow.
- Ordinary arithmetic and CAS arithmetic are related, but not the same mode.

You have reached the end of the introductory book. Return to
[Start Here](00-Start-Here.md) for the reading path or use the reference
documentation when you need the complete language surface.
