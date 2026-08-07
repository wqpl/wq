# A Prime Sieve

This chapter builds the sieve in `e/primes.wq`. The following CAS chapter is
optional.

A sieve keeps a bool mask and crosses out composite numbers.

## The Shape

Create one bool for each number from `0` through the limit.

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

`x+1|iota` creates positions `0` through `x`. The comparison `>1` marks `0`
and `1` as `F` and the remaining positions as `T`.

`p` is the mask. `where p` returns its `T` positions as numbers.

## One Crossing-Out Step

For the prime `2`, cross out every later multiple.

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

The index expression has five steps:

- `j:i^2` starts at `4`.
- `(x-j)/%i+1` counts how many multiples fit from `j` up to `x`.
- `iota` makes the consecutive ints starting at `0` for that count.
- `*i` spaces those numbers by the current prime.
- `+j` shifts them so the first crossed-out number is `j`.

`p[indexes]:F` writes `F` into those positions.

## Why Start At A Square?

For `3`, the crossing-out list starts at `9`.

```wq
x:30
i:3
j:i^2
(x-j)/%i+1|iota|*i|+j
```

Smaller multiples are either the prime itself, such as `3*1`, or have a smaller
factor, such as `3*2`. Starting at `i^2` skips work completed by smaller
factors.

That same idea tells us when to stop:

```wq
x:30
floor sqrt x
```

A composite above this point has a smaller factor that the sieve already
processed.

## The Loop

Move `i` through the possible prime factors.

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

`W[i<=l; ...]` runs through the square-root limit. A `T` at `p i` identifies
the next prime, whose later multiples are then crossed out.

The update `i:$[i=2;3;i+2]` moves from `2` to `3`, then checks odd numbers.

## The Function

The function unpacks the initial mask, limit and first factor, then returns
`where p`.

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

The implicit argument `x` is the upper limit. The repository version in
`e/primes.wq` uses the same function in compact form.

## Summary

- A sieve keeps a bool mask and crosses out composites.
- `where mask` returns the positions that are still `T`.
- `p[indexes]:F` mutates many positions at once.
- Starting at `i^2` skips multiples handled by smaller factors.
- `W` advances through candidate factors up to `sqrt x`.

Continue to [Symbolic Math with CAS](13-CAS.md) if you want the optional
symbolic-math tour. Otherwise, you have completed the introductory path.
