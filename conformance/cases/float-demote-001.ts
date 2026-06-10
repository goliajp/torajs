// float demotion — integer-valued f64 closures rewrite to i64; f64
// semantics must hold exactly on both the fire and must-not-fire
// sides. Shapes are constrained by the pre-existing ann-width family
// (`: number` defaulting to I64 — mixed-width compile aborts and f64
// returns truncated through I64 ret slots; plan-state L3b holds the
// minimal repros). The real demotion fire surface (bounded integer
// closures containing `/`) opens once that family is fixed.

// integer-width domain: no `/` in the function, the frontend lowers
// the whole closure as i64 — interval facts + sext seeds must not
// disturb it and the output is exact.
function oddDown(x: number): number {
  let n: number = x;
  while (n % 2 === 0) {
    n = n - 1;
  }
  return n;
}
let sum: number = 0;
let i: number = 1;
while (i <= 100000) {
  sum = sum + oddDown(i);
  i = i + 1;
}
console.log(sum);

// f64 domain, must NOT fire: non-integer entry — 0.5 keeps the chain
// f64 and the fixed point must print exactly (in-function print: the
// f64-through-i64-ret-slot truncation is the pre-existing debt).
function halfChain(): void {
  let h: number = 0.5;
  let t: number = 0;
  while (t < 10) {
    h = h / 2 + 0.25;
    t = t + 1;
  }
  console.log(h);
}
halfChain();

// f64 domain, must NOT fire: growth loop whose bound is unprovable
// (collatz) — stays f64, trajectory matches bun bit for bit.
function steps(n: number): number {
  let count: number = 0;
  while (n !== 1) {
    if (n % 2 === 0) {
      n = n / 2;
    } else {
      n = 3 * n + 1;
    }
    count = count + 1;
  }
  return count;
}
let maxSteps: number = 0;
let j: number = 1;
while (j <= 5000) {
  const s: number = steps(j);
  if (s > maxSteps) {
    maxSteps = s;
  }
  j = j + 1;
}
console.log(maxSteps);

// top-level fractional flow stays exact
let half: number = 0.5;
console.log(1 / half);
