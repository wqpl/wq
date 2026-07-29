# A Prime Sieve

This chapter closes the core path with one complete little program:
`e/primes.wq`. The following CAS chapter is optional.

It uses a sieve. Instead of asking "is this number prime?" one number at a time, a sieve keeps a yes/no list and crosses out numbers that cannot be prime.

Run each standalone block on its own. When adjacent cells share one Run button,
run the whole group so later cells can use the setup above them.

## The Shape

Start with a limit, then make one bool for each number from `0` through that limit.

<!-- wq-example {"id":"sieve-starting-mask","cellGroup":"sieve-shape"} -->
```wq
x:10
p:x+1|iota|>1
p
```

<!-- wq-example {"id":"sieve-starting-values","cellGroup":"sieve-shape"} -->
```wq
where p
```

`x+1|iota` makes the positions `0` through `x`. The comparison `>1` turns positions `0` and `1` into `F`, and leaves everything else as `T`.

`p` is the mask. It is not a list of prime numbers yet. It is a list of answers to "should the number at this position stay?"

`where p` turns the true positions back into numbers.

## One Crossing-Out Step

If `2` is prime, then every multiple of `2` after `2` is not prime.

<!-- wq-example {"id":"sieve-crossing-indexes","cellGroup":"sieve-crossing"} -->
```wq
x:30
p:x+1|iota|>1
i:2
j:i^2

(x-j)/%i+1|iota|*i|+j
```

<!-- wq-example {"id":"sieve-after-crossing","cellGroup":"sieve-crossing"} -->
```wq
p[(x-j)/%i+1|iota|*i|+j]:F
where p
```

The line that builds the indexes is the densest part:

- `j:i^2` starts at `4`.
- `(x-j)/%i+1` counts how many multiples fit from `j` up to `x`.
- `iota` makes the consecutive ints starting at `0` for that count.
- `*i` spaces those numbers by the current prime.
- `+j` shifts them so the first crossed-out number is `j`.

Then `p[indexes]:F` writes `F` into every crossed-out position.

## Why Start At A Square?

For `3`, the crossing-out list starts at `9`.

```wq
x:30
i:3
j:i^2
(x-j)/%i+1|iota|*i|+j
```

The smaller multiples either are the prime itself, like `3*1`, or have a smaller factor, like `3*2`. Starting at `i^2` avoids doing old work again.

That same idea tells us when to stop:

```wq
x:30
floor sqrt x
```

Once `i` is larger than `sqrt x`, any composite number still left would need a smaller factor too, and that smaller factor already had its turn.

## The Loop

Now let `i` move through the possible prime factors.

```wq
x:30
p:x+1|iota|>1
l:floor sqrt x
i:2

W[i<=l;
  $.[p i;
    j:i^2;
    p[(x-j)/%i+1|iota|*i|+j]:F
  ];
  i:$[i=2;3;i+2]
]

where p
```

`W[i<=l; ...]` keeps going until the square-root limit. Inside it, `p i`
reads the mask at position `i`. If that position is still `T`, every smaller
prime has already had its crossing-out turn, so `i` is prime and its later
multiples can be crossed out.

The update `i:$[i=2;3;i+2]` means "after `2`, check only odd numbers." Even numbers were crossed out on the first pass.

## Pack The Setup

The example file uses unpacking to bind the three setup values at once.

```wq
x:30
(p;l;i):(x+1|iota|>1;floor sqrt x;2)
(where p;l;i)
```

Read that as:

- `p` gets the starting mask.
- `l` gets the square-root limit.
- `i` starts at `2`.

It is the same setup as before, just tighter.

## The Function

Wrap the pieces in a function and return `where p`.

```wq
primes:{
  (p;l;i):(x+1|iota|>1;floor sqrt x;2)

  W[i<=l;
    $.[p i;
      j:i^2;
      p[(x-j)/%i+1|iota|*i|+j]:F
    ];
    i:$[i=2;3;i+2]
  ]

  where p
}

primes 30
```

The function uses the implicit argument `x`, so `primes 30` means "all primes up to 30."

## The Example File

The original `e/primes.wq` is the same program written compactly:

```wq
primes:{(p;l;i):(x+1|iota|>1;floor sqrt x;2)
  W[i<=l;$.[p i;j:i^2;p[(x-j)/%i+1|iota|*i|+j]:F];i:$[i=2;3;i+2]];where p}
primes[100][-10..0]
```

`primes[100]` makes all primes up to `100`. `[-10..0]` takes the last ten positions, so the final line shows the ten largest primes under `100`.

## Keep

- A sieve keeps a bool mask and crosses out composites.
- `where mask` returns the positions that are still `T`.
- `p[indexes]:F` mutates many positions at once.
- Starting at `i^2` avoids crossing out numbers smaller factors already handled.
- For a small complete program, write the readable version first, then tighten it only where the shape stays clear.

Continue to [Symbolic Math with CAS](13-CAS.md) if you want the optional
symbolic-math tour. Otherwise, you have completed the introductory path.
