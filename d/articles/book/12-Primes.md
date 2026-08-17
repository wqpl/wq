# A Prime Sieve

This chapter builds the sieve in `e/primes.wq`. The following CAS chapter is
optional.

A sieve keeps a bool mask and crosses out composite numbers.

## The Mask

Create one bool for each number from `0` through the limit.

<!-- wq-example {"id":"sieve-starting-mask","cellGroup":"sieve-shape"} -->
```wq
x:10
p:0..=x>1
p
```

<!-- wq-example {"id":"sieve-starting-values","cellGroup":"sieve-shape"} -->
```wq
where p
```

`0..=x` includes both ends of the range. The comparison `>1` marks `0` and `1`
as `F` and every later position as `T`.

`p` is the mask. `where p` returns its `T` positions as numbers.

## One Crossing-Out Step

For the prime `2`, cross out every multiple from its square onward.

<!-- wq-example {"id":"sieve-crossing-indexes","cellGroup":"sieve-crossing"} -->
```wq
x:30
p:0..=x>1
i:2

range[i^2;#p;i]
```

<!-- wq-example {"id":"sieve-after-crossing","cellGroup":"sieve-crossing"} -->
```wq
p[range[i^2;#p;i]]:F
where p
```

`range[start;end;step]` is half-open. Here it starts at `i^2`, stops before
`#p`, and advances by `i`. Since the mask has `x+1` positions, the resulting
indexes include every multiple up to `x`.

`p[indexes]:F` writes `F` into all of those positions at once.

## Why Start At A Square?

For `3`, the crossing-out range starts at `9`.

```wq
x:30
p:0..=x>1
i:3
range[i^2;#p;i]
```

Smaller multiples are either the prime itself, such as `3*1`, or have a smaller
factor, such as `3*2`. Starting at `i^2` skips work completed by smaller
factors.

The same square tells the loop when to stop. The condition `i^2<#p` is true
exactly while `i^2<=x`, because the mask length is `x+1`.

## Skip Composite Factors

Only positions still marked `T` need a crossing-out step.

```wq
x:30
p:0..=x>1
i:2

A[p i;p[range[i^2;#p;i]]:F]
where p
```

`A[...]` is short-circuit bool and. When `p i` is `F`, it does not evaluate the
assignment. When `p i` is `T`, the assignment crosses out that factor's
multiples and returns the assigned bool `F`. The loop ignores the final bool.

This lets the loop visit consecutive ints. Composite factors cost only the
mask check.

## The Loop

Move `i` through the possible factors.

```wq
x:30
p:0..=x>1
i:2

W[i^2<#p;
  A[p i;
    p[range[i^2;#p;i]]:F];
  i+:1]

where p
```

`W[i^2<#p; ...]` stops after every composite up to `x` has a smaller factor
that was already processed. `i+:1` advances to the next candidate. The
short-circuit check prevents composite candidates from repeating the work.

## The Function

The function receives the inclusive upper limit as its implicit argument `x`
and returns the positions left marked `T`.

```wq
primes:{
  p:0..=x>1;
  i:2;
  W[i^2<#p;
    A[p i;
      p[range[i^2;#p;i]]:F];
    i+:1];
  where p}

primes 30
```

The compact repository version is:

```wq
{p:0..=x>1;i:2;W[i^2<#p;A[p i;p[range[i^2;#p;i]]:F];i+:1];where p}
```

The file returns the function directly. An import binds that function to the
name chosen by the importing code.

## Summary

- A sieve keeps a bool mask and crosses out composites.
- `0..=x>1` builds the initial mask directly from an inclusive range.
- `range[i^2;#p;i]` selects every remaining multiple of a factor.
- `p[indexes]:F` mutates many positions at once.
- `A[...]` skips the mutation when the current factor is composite.
- `i^2<#p` keeps the loop inside the square-root boundary.
- `where p` returns the prime positions.

Continue to [Symbolic Math with CAS](13-CAS.md) if you want the optional
symbolic-math tour. Otherwise, you have completed the introductory path.
