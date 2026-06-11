// ann-width W5 — growth-cycle slots must keep the F64 representation
// (rfc 20260611-ann-width-unification §5.5, repro S7). Integral-only
// self-feeding chains used to narrow to i64 and wrap at 2^63 where
// bun's f64 semantics round from 2^53 on. Every section is `/`-free
// on purpose: the only f64 seed is the growth cycle itself.

// S7: geometric self-feeding loop — must round like f64, not wrap.
function grow(start: number, steps: number): number {
  let n: number = start;
  let i: number = 0;
  while (i < steps) {
    n = n * 3 + 1;
    i = i + 1;
  }
  return n;
}
console.log(grow(1, 40));
console.log(grow(1, 90)); // deep past 2^53 — pure f64 rounding tail

// cross-slot cycle: the growth feeds back through a temp.
function relay(steps: number): number {
  let n: number = 1;
  let t: number = 0;
  let i: number = 0;
  while (i < steps) {
    t = n * 3;
    n = t + 1;
    i = i + 1;
  }
  return n;
}
console.log(relay(45));

// additive cycle with slot operands: fib grows past 2^53 by step 79.
function fibIter(k: number): number {
  let a: number = 0;
  let b: number = 1;
  let i: number = 0;
  while (i < k) {
    let t: number = a + b;
    a = b;
    b = t;
    i = i + 1;
  }
  return a;
}
console.log(fibIter(79));

// recursive growth cycle: the ret slot feeds itself through `+`.
function fib(n: number): number {
  if (n < 2) {
    return n;
  }
  return fib(n - 1) + fib(n - 2);
}
console.log(fib(35));

// non-const accumulator: the step is a slot, not a literal.
function pile(k: number, c: number): number {
  let acc: number = 1;
  let i: number = 0;
  while (i < k) {
    acc = acc + c;
    i = i + 1;
  }
  return acc;
}
console.log(pile(12, 900719925474099));
