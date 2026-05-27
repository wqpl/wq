# Conditionals

A **conditional** lets you run code only when a condition is met.

wq conditionals are expressions. This means every conditional yields a value, allowing you to pass it to a function, assign it to a variable, or use it directly in a calculation.

This expression-first philosophy reduces verbose `if/else` blocks found in some programming languages and avoids temporary variables.

## Booleans

A **boolean** represents a simple truth value: true or false.

To combine or invert booleans, wq provides three built-in functions: `and`, `or`, and `not`.
Note that wq does not use symbols like `&&` or `!` for boolean operations, unlike some other languages.

```wq
not true          |echo
and[true;false]   |echo
or[false;true]    |echo
```

## Overview

wq has three conditional forms: ternary (`$`), guard (`$.`), and chain (`$$`).

- ternary: `$[cond;true;false;...]`
- guard: `$.[cond;true;...]`
- chain: `$$[c1;t1;c2;t2;...;def]`

## Ternary `$[c;t;f;...]`

Use the ternary to choose between a true branch and a false branch.

- It requires at least 3 parts: `cond`, `true`, and `false`.
- `cond` is evaluated first.
- If `cond` is true, the true branch is evaluated and returned.
- If `cond` is false, expressions of the false branch are evaluated in order, and the last value is returned.

```wq
r:rand[]
echo $[r<0.5;"<0.5";">=0.5"]

// the false side can do more work
echo $[r<0.5;"<0.5";
  echo @f"not <0.5, r={r}";">=0.5"]
```

Note again that for the ternary, the true branch can only contain a single expression. However, the false branch can contain as many expressions as you want.

## Guard `$.[c;t;...]`

Use the guard when you only care about the `true` case.

- It requires at least 2 parts: `cond` and `true`.
- If `cond` is true, all parts starting from the second are evaluated, and the last value is returned.
- If `cond` is false, the expression yields the unit value `()`.

```wq
DEBUG:true
a:rand[-1.0;1.0]
$.[DEBUG;echo @f"Debug: a={a}"]
echo abs a
```

## Chain `$$[c1;t1;...;d]`

Use the chain to handle multiple cases without having to write messy, nested ternary expressions.

- Conditions are tested from left to right.
- The first matching condition selects its paired result.
- The default branch `def` is optional (omit it if you want the chain to yield `()` when nothing matches).

```wq
score:rand[0;100]
grade: $$[
  score>=90;"a";
  score>=80;"b";
  score>=70;"c";
  "f"]
echo grade
```

## Summary

- `$[...]` chooses between a true branch and a false branch.
- `$.[...]` executes only if true; otherwise returns ().
- `$$[...]` checks multiple conditions from left to right and returns the first match.
- Every conditional is an expression that evaluates to a value.
