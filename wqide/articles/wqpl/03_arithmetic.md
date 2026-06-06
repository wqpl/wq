# Arithmetic

wq supports ordinary numeric expressions, elementwise list arithmetic, and a useful set of math bfns.

## Basic operators

| Operator | Functionality   |
| -------- | --------------- |
| +        | add             |
| -        | subtract/negate |
| \*       | multiply        |
| /        | divide          |
| %        | modulo          |
| ^        | exponent        |

```wq
1+2   |echo
10-3  |echo
6*7   |echo
7/2   |echo
7%2   |echo
2^5   |echo
-5+-3 |echo
```

wq evaluates operators based on precedence: Exponents (`a^b`) are evaluated first, followed by negation (`-x`), multiplication/division (`a*b`), (`a/b`), and finally addition/subtraction (`a+b`, `a-b`).

| Precedence | Operator                         |
| ---------- | -------------------------------- |
| 1          | ^ (exponent)                     |
| 2          | - (negation)                     |
| 3          | \* (multiplication) / (division) |
| 4          | + (addition) - (subtraction)     |

Use parentheses when you want to make grouping explicit:

```wq
2*(3+4)
```

Division always produces float results, even for integer inputs:

```wq
8/4
```

You get an error when you divide by zero.

```wq
1/0
```

```wq
1%0
```

The exponent operator is right-associative; `2^3^2` means `2^(3^2)`.

```wq
2^3^2   |echo
2^(3^2) |echo
(2^3)^2 |echo
```

## Dotted division and modulo

The dotted variants, `/.` and `%.`, do not raise an error when you divide by zero. Instead, they produce `inf`, `-inf`, or `nan`, following IEEE floating-point rules.

```wq
17/.5 |echo
17%.5 |echo
```

```wq
7/.2  |echo
7%.2  |echo
1/.0  |echo
0/.0  |echo
-1/.0 |echo
0%.0  |echo
```

## Bigints

In wq, integers can grow infinitely large.

```wq
a:2^63-1
echo a
echo type a
echo "---"

a:2^64
echo a
echo type a
```

## Comparison operators

Common comparison operators are:

| Operator | Functionality         |
| -------- | --------------------- |
| =        | equal                 |
| ~        | not equal             |
| <        | less than             |
| <=       | less than or equal    |
| >        | greater than          |
| >=       | greater than or equal |

Notice that the "not equal" operator is `~`, rather than the `!=` commonly used in other languages.

```wq
3<5   |echo
3=3   |echo
8>10  |echo
1~1.0 |echo
```

### Comparison chains

Comparison chains read naturally in sequence.

Hint: use a semicolon ; to separate multiple expressions on a single line.

```wq
1<2<3       |echo
x:4;10>=x>5 |echo
```

### Broadcasting

`<`, `<=`, `>`, `>=` broadcasts (operates elementwise):

```wq
(1;2;3)>=(10;-20;30)    |echo
(1;2;3)<(10;20;30)      |echo
```

`=` and `~` do not broadcast. They compare lists as whole objects and return a single boolean.

```wq
(1;2;3)=(4;5;6) |echo
(1;2;3)~(4;5;6) |echo
```

## Useful math bfns

wq includes a rich set of built-in functions to handle everyday operations.

### Passing arguments

To pass multiple arguments to a function, use square brackets `[]` and separate the arguments with a semicolon `;`.
However, if you are only passing exactly one argument, you can omit the square brackets.

### Elementary math

```wq
sin 0      |echo
cos 0      |echo
ln 10      |echo
ceil 3.14  |echo
floor 3.14 |echo
```

```wq
sum(1;2;3) |echo
max[10;20] |echo
max(10;20) |echo
min[10;20] |echo
```

The difference between `max[10;20]` and `max(10;20)`:

- `max[10;20]` uses brackets to pass two separate arguments (the numbers `10` and `20`).
- `max(10;20)` omits the brackets and passes one single argument: the list `(10;20)`.
  - It is equivlent to `max[(10;20)]`.

Certain functions like `max` are smart enough to handle both, but it is important to remember that `[]` passes multiple arguments, while `()` simply creates a list.

### Randomness

```wq
rand[]         |echo
rand[0;10]     |echo
rand[-1.0;1.0] |echo
```

## Summary

- Basic operators:
  - `+-*/%^`
  - `/.` and `%.`
- Integers can grow infinitely big.
- Arithmetic works on both numbers and lists.
