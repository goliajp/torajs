// float demotion phase 1b-ii — loop versioning side exits. A guarded
// growth site hands the fast i64 path off to the preserved f64 loop
// exactly where the trajectory leaves the ±(2^53-1) window; from
// there the f64 slow path must reproduce bun's rounding tail bit for
// bit. (The `/` in each loop is load-bearing: it seeds the f64 width
// so the demotion pipeline owns the loop; single call sites keep the
// helpers inlinable so the interval analysis sees the constant
// entries. Sections printing the trajectory value itself stay f64 —
// a post-loop SCC use escapes any hostable region — and pin the
// ground-truth path; sections printing the step count demote and pin
// the fast path + side exits.)

// f64 ground truth across 2^53: 3n+1 from just above the guard
// window rounds 3n to even — the printed tail comes from the rounded
// halvings.
function cross(seed: number, steps: number): number {
  let n: number = seed;
  let i: number = 0;
  while (i < steps) {
    if (n % 2 === 0) {
      n = n / 2;
    } else {
      n = n * 3 + 1;
    }
    i = i + 1;
  }
  return n;
}
console.log(cross(3002399751580331, 6));

// short trajectory stays inside the window: the fast i64 path runs
// to completion and the result is exact.
let small: number = 7;
let s: number = 0;
while (s < 20) {
  if (small % 2 === 0) {
    small = small / 2;
  } else {
    small = small * 3 + 1;
  }
  s = s + 1;
}
console.log(small);

// upper-bound guard + side exit: an odd-only climb (the even arm
// never runs but keeps the f64 seed) with an fcmp loop exit — the
// fast i64 path runs ~30 steps, the side exit fires past
// (2^53-1)/3, and the f64 slow path carries the loop to its bound.
function climb(start: number): number {
  let n: number = start;
  let i: number = 0;
  while (n < 9000000000000000) {
    if (n % 2 === 0) {
      n = n / 2;
    } else {
      n = n * 3 - 2;
    }
    i = i + 1;
  }
  return i;
}
console.log(climb(3));

// lower-bound guard + side exit: the negative mirror — the side
// exit fires when n drops below -(2^53-1)/3 and the f64 slow path
// carries the loop to its bound.
function sink(start: number): number {
  let n: number = start;
  let i: number = 0;
  while (n > -9000000000000000) {
    if (n % 2 === 0) {
      n = n / 2;
    } else {
      n = n * 3 - 2;
    }
    i = i + 1;
  }
  return i;
}
console.log(sink(-1));
