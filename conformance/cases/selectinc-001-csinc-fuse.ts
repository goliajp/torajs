// conditional-increment fuse (RFC 20260719-select-formation residual
// "csinc 特化"): after select formation, `if (pred) { n = n + 1 }`
// arrives as `icmp; add n,1; select c, add, n`, which collapses to
// `cmp; csinc` — the CSET, the ADD and the `cmp #0` all disappear.
//
// The rewrite is only sound when the ADD has a single reader (and the
// NZCV fuse only when the compare does too), so each guard gets a
// case that must still produce the plain arithmetic answer.

// increment on the then arm — the common counting shape
function countOdd(n: number): number {
  let c: number = 0;
  let i: number = 0;
  while (i < n) {
    if (i % 2 === 1) {
      c = c + 1;
    }
    i = i + 1;
  }
  return c;
}
console.log(countOdd(1000));
console.log(countOdd(1001));
console.log(countOdd(0));

// increment on the else arm — mirrored polarity, no predicate flip
function countNotThree(n: number): number {
  let c: number = 0;
  let i: number = 0;
  while (i < n) {
    if (i % 3 === 0) {
      c = c;
    } else {
      c = c + 1;
    }
    i = i + 1;
  }
  return c;
}
console.log(countNotThree(100));

// the incremented value is read again — the ADD cannot be dropped, so
// this must fall back to the plain add + select
function countAndCarry(n: number): number {
  let c: number = 0;
  let last: number = 0;
  let i: number = 0;
  while (i < n) {
    const bumped: number = c + 1;
    if (i % 2 === 1) {
      c = bumped;
    }
    last = bumped;
    i = i + 1;
  }
  return c * 1000 + last;
}
console.log(countAndCarry(50));

// the condition is read again — the ADD still folds but the compare
// keeps its CSET, so the CSINC runs off `cmp cond, #0`
function countAndFlag(n: number): number {
  let c: number = 0;
  let flags: number = 0;
  let i: number = 0;
  while (i < n) {
    const odd: boolean = i % 2 === 1;
    if (odd) {
      c = c + 1;
    }
    flags = flags + (odd ? 1 : 0);
    i = i + 1;
  }
  return c * 1000 + flags;
}
console.log(countAndFlag(50));

// two independent counters in one body — both fuse, neither steals the
// other's predicate
function countBoth(n: number): number {
  let a: number = 0;
  let b: number = 0;
  let i: number = 0;
  while (i < n) {
    if (i % 2 === 0) {
      a = a + 1;
    }
    if (i % 5 === 0) {
      b = b + 1;
    }
    i = i + 1;
  }
  return a * 1000 + b;
}
console.log(countBoth(100));

// counting down through a decrement must NOT be treated as +1
function countDown(n: number): number {
  let c: number = 100;
  let i: number = 0;
  while (i < n) {
    if (i % 2 === 1) {
      c = c - 1;
    }
    i = i + 1;
  }
  return c;
}
console.log(countDown(50));

// a non-unit step is not an increment either
function countByTwo(n: number): number {
  let c: number = 0;
  let i: number = 0;
  while (i < n) {
    if (i % 2 === 1) {
      c = c + 2;
    }
    i = i + 1;
  }
  return c;
}
console.log(countByTwo(50));
