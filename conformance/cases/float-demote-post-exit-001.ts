// float_demote's post-op growth guard checks an accumulator add/sub
// after the i64 op ran, and its side exit enters the preserved f64
// loop after the op with the i64 result written back through sitofp.
// The result is the exact sum of two values inside ±2^53, so that one
// rounding must equal the rounding the f64 op itself makes; past the
// exit every step rounds in f64 as bun's does. One function per loop
// keeps each accumulator in a region of its own.

// odd steps: the sum crosses 2^53 and the tail rounds to even
function up(n: number): number {
  let a = 0;
  for (let i = 0; i < n; i++) a += (i & 1023) * 3000000007 + 1;
  return a;
}
console.log(up(20000));

// the same descent through -2^53
function down(n: number): number {
  let b = 0;
  for (let i = 0; i < n; i++) b -= (i & 1023) * 3000000007 + 1;
  return b;
}
console.log(down(20000));

// a step of either sign: the guard checks both sides
function drift(n: number): number {
  let c = 0;
  for (let i = 0; i < n; i++) c += (i & 1023) * 3000000007 - 2000000000001;
  return c;
}
console.log(drift(20000));

// the old value stays live past the add: once the sum sits at 2^53,
// adding 1 rounds back to it
function stuck(n: number): string {
  let e = 9007199254740000;
  let same = 0;
  for (let i = 0; i < n; i++) {
    const before = e;
    e += i & 1;
    if (e === before) same++;
  }
  return e + " " + same;
}
console.log(stuck(2000));

// the exit fires on the very first iteration
function edge(n: number): number {
  let f = 9007199254740991;
  for (let i = 0; i < n; i++) f += (i & 3) + 1;
  return f;
}
console.log(edge(5));
