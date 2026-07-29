# Arithmetic

Arithmetic is the easiest place to feel wq's personality: small expressions,
no ceremony, and lists that do math with you.

## Calculator Mode

The familiar operators are here. `^` means power.

<!-- wq-example {"id":"calculator-precedence","cellGroup":"calculator-mode","expect":{"value":"7"}} -->
```wq
1+2*3
```

<!-- wq-example {"id":"calculator-parentheses","cellGroup":"calculator-mode","expect":{"value":"9"}} -->
```wq
(1+2)*3
```

<!-- wq-example {"id":"calculator-power","cellGroup":"calculator-mode","expect":{"value":"256"}} -->
```wq
2^8
```

Multiplication binds more tightly than addition. Parentheses make grouping
explicit. Postfix calls have their own tighter precedence, which the Calls
chapter covers before you need it.

Power groups from the right:

<!-- wq-example {"id":"power-right","cellGroup":"power-grouping","expect":{"value":"512"}} -->
```wq
2^3^2
```

<!-- wq-example {"id":"power-left","cellGroup":"power-grouping","expect":{"value":"64"}} -->
```wq
(2^3)^2
```

So the first line is `2^(3^2)`, not `(2^3)^2`.

## Division Has Flavors

Division operators differ in the result they preserve:

| Operator | Meaning | `7` and `2` produce |
| --- | --- | --- |
| `/` | Floating division | `3.5` |
| `/%` | Floor division | `3` |
| `%` | Remainder | `1` |
| `/.` | Exact division | `7/2` |

<!-- wq-example {"id":"division-float","cellGroup":"division-flavors","expect":{"value":"3.5"}} -->
```wq
7/2
```

<!-- wq-example {"id":"division-floor","cellGroup":"division-flavors","expect":{"value":"3"}} -->
```wq
7/%2
```

<!-- wq-example {"id":"division-remainder","cellGroup":"division-flavors","expect":{"value":"1"}} -->
```wq
7%2
```

<!-- wq-example {"id":"division-exact","cellGroup":"division-flavors","expect":{"value":"7/2"}} -->
```wq
7/.2
```

Divide by zero and wq stops at the failing expression. Expect a `zero-div`
error saying `cannot divide by zero`.

<!-- wq-example {"id":"divide-by-zero","expect":{"error":"zero-div"}} -->
```wq
1/0
```

## Power Has Flavors Too

`^` is the everyday runtime power operator. Positive integer powers stay exact when they can, but negative or fractional numeric powers use classic floating-point arithmetic.

<!-- wq-example {"id":"classic-power-int","cellGroup":"classic-power","expect":{"value":"256"}} -->
```wq
2^8
```

<!-- wq-example {"id":"classic-power-negative","cellGroup":"classic-power","expect":{"value":"0.125"}} -->
```wq
2^-3
```

<!-- wq-example {"id":"classic-power-fractional","cellGroup":"classic-power","expect":{"value":"0.6666666666666666"}} -->
```wq
(8/.27)^(1/.3)
```

`^.` asks for exact exponentiation. Pair it with exact operands such as `/.` when the exponent is fractional:

<!-- wq-example {"id":"exact-power-negative","cellGroup":"exact-power","expect":{"value":"1/8"}} -->
```wq
2^.-3
```

<!-- wq-example {"id":"exact-power-fractional","cellGroup":"exact-power","expect":{"value":"2/3"}} -->
```wq
(8/.27)^.(1/.3)
```

## Lists Join In

A list is written with parentheses and semicolons:

```wq
(1;2;3)
```

The fun part is that arithmetic works through the shape of a value.

<!-- wq-example {"id":"broadcast-atom","cellGroup":"broadcasting","expect":{"value":"(11;12;13)"}} -->
```wq
(1;2;3)+10
```

<!-- wq-example {"id":"broadcast-matching","cellGroup":"broadcasting","expect":{"value":"(10;200;3000)"}} -->
```wq
(1;2;3)*(10;100;1000)
```

<!-- wq-example {"id":"broadcast-nested","cellGroup":"broadcasting","expect":{"value":"((11;12);(23;24))"}} -->
```wq
((1;2);(3;4))+(10;20)
```

That is broadcasting: an atom can flow across a list, and matching lists combine item by item.

Mismatched shapes are not guessed at. Expect a `length` error that reports
both list lengths.

<!-- wq-example {"id":"broadcast-length","expect":{"error":"length"}} -->
```wq
(1;2;3)+(10;20)
```

## Plus Is Not Glue

`+` adds. The comma `,` concatenates.

<!-- wq-example {"id":"lists-concatenate","cellGroup":"plus-versus-comma","expect":{"value":"(1;2;3;4)"}} -->
```wq
(1;2),(3;4)
```

<!-- wq-example {"id":"lists-add","cellGroup":"plus-versus-comma","expect":{"value":"(4;6)"}} -->
```wq
(1;2)+(3;4)
```

That distinction matters early. Once you trust it, list code becomes easier to read: math looks like math, joining looks like joining.

## Tiny Curves

Because arithmetic broadcasts, a list can stand in for a little row of inputs.

```wq
(0;1;2;3;4;5)^2
```

You can pipe the result into a builtin too. Here `sum` adds the squared values:

```wq
(1;2;3;4;5)^2|sum
```

It is a small thing, but this is the shape of a lot of wq: make a value, transform it, pass it along.

## Number Questions

Comparisons produce bools, `T` or `F`. A comparison chain checks each adjacent
pair; a list comparison broadcasts.

<!-- wq-example {"id":"comparison-chain","cellGroup":"number-questions","expect":{"value":"T"}} -->
```wq
1<2<3
```

<!-- wq-example {"id":"comparison-broadcast","cellGroup":"number-questions","expect":{"value":"(F;T;T)"}} -->
```wq
(1;2;3)>1
```

Plain `=` compares whole values. Dotted `=.` compares through matching leaves:

<!-- wq-example {"id":"comparison-whole","cellGroup":"whole-versus-leaves","expect":{"value":"F"}} -->
```wq
(1;2;3)=(1;9;3)
```

<!-- wq-example {"id":"comparison-leaves","cellGroup":"whole-versus-leaves","expect":{"value":"(T;F;T)"}} -->
```wq
(1;2;3)=.(1;9;3)
```

## Keep

- `+ - * / % ^` are the everyday arithmetic operators.
- `/%` is floor division; `/.` is exact division; `^.` is exact power.
- Arithmetic broadcasts over compatible list shapes.
- `,` concatenates; `+` never means concatenate.
- Continue to **Binding** to name and update values.
