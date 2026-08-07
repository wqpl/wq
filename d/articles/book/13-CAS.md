# Symbolic Math with CAS

This optional chapter introduces wq's computer algebra system (CAS). The core
introductory path ends before it.

`@s` creates symbolic math as a CAS value.

```wq
expr:@s x^2+2*x+1
expr
```

Ordinary evaluation resolves `x` as a binding. `@s` quotes the expression for
symbolic work.

## Transforming an Expression

Apply CAS functions to the symbolic value.

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

The symbolic value moves through ordinary pipes. Named arguments bind symbolic
variables in CAS functions and CAS values. A single-variable CAS expression
also accepts one positional argument.

## Factoring and Solving

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

| Call shape | Result |
| --- | --- |
| `solve[expression]` | Roots for one inferred variable |
| `solve[expression;variable]` | Roots for the selected variable |
| `solve_system[equations]` | A dict keyed by inferred variable names |
| `solve_system[equations;variables]` | A dict in the requested variable order |

Supported linear and quadratic solves retain other symbols as parameters.

When a symbolic coefficient can change a polynomial's degree or a system's
rank, the solver returns cases. Each case has a `when` list and a branch result.
Assumptions narrow those cases:

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

The CAS derives consequences such as positive values being nonzero and ints
being real. Contradictory assumptions produce an error.

Read solver results as follows:

- For `solve`, a finite root set is a list, the `` `all `` tag means every
  value is a solution, and an empty list means there is no solution.
- The default domain is the `` `complex `` tag. Pass the named argument
  `domain` with the `` `real `` tag to exclude non-real roots.
- A parameterized real-domain solve requires `real` assumptions for its
  symbolic coefficients.
- For `solve_system`, a unique solution is a variable dict, the `` `none `` tag
  means no solution, and dependent systems return a `` `solution `` dict with a
  `parameters` list of fresh symbols.

## Numeric Evaluation

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

The roots support list arithmetic. `numeric` binds `x` and `y` before evaluating
`f`. It is the explicit boundary from exact symbolic constants and functions to
approximations.

## Summary

- `@s expr` creates a symbolic expression.
- After `@s`, pass the symbolic value to CAS functions.
- Pipes remain outside the symbolic quote, so `@s x^2|diff` uses ordinary value
  flow.
- `numeric` crosses from symbolic values into approximation.

You have reached the end of the introductory book. Return to
[Start Here](00-Start-Here.md) for the reading path or use the reference
documentation when you need the complete language surface.
