// RFC 20260726-new-on-function blade 1 — a function may mention `this`.
// Declaring one used to be a compile error ("unknown identifier
// __this"), whether or not it was ever used as a constructor.

function F(x: number) { this.x = x; }
console.log(typeof F);

function G(a: string) { this.a = a; }
function H() { this.n = 1; }
console.log(typeof G, typeof H);

// `this` on a branch that is not taken: a plain call stays safe, which
// is what lets the rest of this fixture call these at all.
function P(flag: boolean, v: number): number {
  if (flag) { this.v = v; return 0; }
  return v * 2;
}
console.log(P(false, 21));

// The hidden parameter goes in front of the declared ones, so every
// call site gains an argument. These check the arguments still line up
// with the parameters — a shifted-by-one bug would print garbage here
// rather than fail loudly.
function R(n: number): number {
  if (n > 100) { this.big = true; }
  return n + 1;
}
console.log(R(1), R(2), R(3));

function S(a: number, b: number): number {
  if (a < 0) { this.neg = true; }
  return a + b;
}
console.log(S(3, 4), S(10, 20));

// A function that mentions `this` calling another one.
function Inner(v: number): number {
  if (v === 999) { this.sentinel = v; }
  return v * 3;
}
function Outer(v: number): number {
  if (v === 998) { this.other = v; }
  return Inner(v) + 1;
}
console.log(Outer(5));
