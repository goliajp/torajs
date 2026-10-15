// Explicit type arguments on a call through `(a, f)<T>(x)` or
// `(c ? f : g)<T>(x)` instantiate whichever function the callee
// produces, the same as on a direct call.
function mk<T>(): T[] {
  return [];
}
function mk2<T>(): T[] {
  const out: T[] = [];
  return out;
}
function wrap<T>(x: T): { v: T } {
  return { v: x };
}
function wrap2<T>(x: T): { v: T } {
  return { v: x };
}
let c = false;
const a = (c ? mk : mk2)<string>();
a.push("x");
a.push("y");
console.log(a.join(","), a.length);
c = true;
const b = (c ? mk : mk2)<number>();
b.push(1.5);
b.push(2);
console.log(b[0] + b[1], b.length);
const w = (c ? wrap : wrap2)<number>(41);
console.log(w.v + 1);
const s = (0, wrap)<string>("s");
console.log(s.v + "!");
c = false;
const d = (c ? wrap : wrap2)<number[]>([1, 2, 3]);
console.log(d.v.length, d.v[2]);
