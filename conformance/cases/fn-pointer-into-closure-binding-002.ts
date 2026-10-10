// stores into closure bindings whose slot no declaration spells, a
// parameter sharing the binding's name, and a choice of closures whose
// number widths were settled by another binding
function h(x: number): number {
  return x * 2;
}
function inc(x: number): number {
  return x + 1;
}
function mk(n: number): (x: number) => number {
  return (x: number) => x + n;
}

// the slot comes from a call
let t = mk(1);
console.log(t(10));
t = h;
console.log(t(10), t === h);
t = Number("1") > 0 ? inc : h;
console.log(t(10), t === inc);

// a parameter and a local closure binding with the same name
function apply(cb: (x: number) => number, v: number): number {
  const first = cb(v);
  cb = Number("1") > 0 ? inc : h;
  return first * 100 + cb(v);
}
function f1(v: number): number {
  let cb = (x: number) => x + v;
  const a = cb(1);
  cb = h;
  const b = cb(1);
  cb = v > 0 ? inc : h;
  return a * 100 + b * 10 + cb(1);
}
console.log(apply(h, 3), f1(4));

// a choice of functions whose widths another binding widened
let op = h;
op = (x: number) => -x;
console.log(op(4));
let g = (x: number) => x;
g = Number("1") > 0 ? h : inc;
console.log(g(10));

// a choice of closures stored next to an array that is mapped
const fs: ((x: number) => number)[] = [];
let k = (x: number) => x;
fs.push(k);
k = Number("1") > 0 ? (x: number) => x * 3 : (x: number) => x * 5;
console.log(k(10));
fs.push(k);
console.log(fs.map((f) => f(10)).join(","));
