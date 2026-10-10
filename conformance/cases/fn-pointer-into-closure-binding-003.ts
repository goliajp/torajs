// a `var` binding and a variadic-function binding, each reassigned a
// top-level function: calls through them run what they hold now
function h(x: number): number {
  return x * 2;
}
function inc(x: number): number {
  return x + 1;
}
function mk(k: number): (x: number) => number {
  return (x: number) => x + k;
}
var t = mk(1);
console.log(t(10));
t = h;
console.log(t(10));
function sum(...xs: number[]): number {
  let s = 0;
  for (const x of xs) s += x;
  return s;
}
function prod(...xs: number[]): number {
  let p = 1;
  for (const x of xs) p *= x;
  return p;
}
let v: (...xs: number[]) => number = sum;
console.log(v(2, 3, 4));
v = prod;
console.log(v(2, 3, 4));
function local(): number {
  var f = mk(5);
  const a = f(1);
  f = inc;
  return a * 100 + f(1);
}
console.log(local());
