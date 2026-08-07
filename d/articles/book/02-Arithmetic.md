# Arithmetic

Arithmetic uses familiar operators and broadcasts across compatible lists.

## Calculator Mode

`^` means power.

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

Multiplication binds more tightly than addition. Parentheses set explicit
grouping. The Calls chapter covers the tighter precedence of postfix calls.

Power groups from the right:

<!-- wq-example {"id":"power-right","cellGroup":"power-grouping","expect":{"value":"512"}} -->
```wq
2^3^2
```

<!-- wq-example {"id":"power-left","cellGroup":"power-grouping","expect":{"value":"64"}} -->
```wq
(2^3)^2
```

The first line groups as `2^(3^2)`. Parentheses produce `(2^3)^2`.

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

Division by zero stops at the failing expression with a `zero-div` error.

<!-- wq-example {"id":"divide-by-zero","expect":{"error":"zero-div"}} -->
```wq
1/0
```

## Power Has Flavors Too

`^` uses runtime numeric arithmetic. Positive int powers stay exact when
possible. Negative and fractional numeric powers can produce floats or complex
values.

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

`^.` performs exact exponentiation. Use exact operands such as `/.` for an
exact fractional exponent:

<!-- wq-example {"id":"exact-power-negative","cellGroup":"exact-power","expect":{"value":"1/8"}} -->
```wq
2^.-3
```

<!-- wq-example {"id":"exact-power-fractional","cellGroup":"exact-power","expect":{"value":"2/3"}} -->
```wq
(8/.27)^.(1/.3)
```

## Broadcasting

Arithmetic follows the shape of list operands.

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

An atom broadcasts across a list. Matching lists combine item by item.

List operands require compatible shapes. A mismatch produces a `length` error
that reports both lengths.

<!-- wq-example {"id":"broadcast-length","expect":{"error":"length"}} -->
```wq
(1;2;3)+(10;20)
```

## Addition and Concatenation

`+` adds. The comma `,` concatenates.

<!-- wq-example {"id":"lists-concatenate","cellGroup":"plus-versus-comma","expect":{"value":"(1;2;3;4)"}} -->
```wq
(1;2),(3;4)
```

<!-- wq-example {"id":"lists-add","cellGroup":"plus-versus-comma","expect":{"value":"(4;6)"}} -->
```wq
(1;2)+(3;4)
```

Broadcasting also applies before a pipe:

```wq
(1;2;3;4;5)^2|sum
```

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

## Summary

- `+ - * / % ^` are the everyday arithmetic operators.
- `/%` is floor division. `/.` is exact division. `^.` is exact power.
- Arithmetic broadcasts over compatible list shapes.
- `+` adds. `,` concatenates.
- Continue to **Binding** to name and update values.
