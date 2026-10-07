// Object literals with the same field names, only one of whose
// accessors reads `arguments`: each answers with its own accessor, on
// [[Get]], [[Set]], JSON.stringify, spread, Object.values, and through
// a value that may be either literal.

const a = {
  n: 1,
  get g() { return this.n + arguments.length * 100; },
  set s(v: number) { this.n = v + arguments.length * 10; },
};
const b = {
  n: 2,
  get g() { return this.n; },
  set s(v: number) { this.n = v; },
};
a.s = 5;
b.s = 6;
console.log(a.g, b.g, a.n, b.n);
console.log(JSON.stringify(a), JSON.stringify(b));
console.log(JSON.stringify({ ...a }), JSON.stringify({ ...b }));
console.log(JSON.stringify(Object.values(a)), JSON.stringify(Object.values(b)));
const pick = (k: number) => (k > 0 ? a : b);
console.log(pick(1).g, pick(0).g);

function mk(k: number) {
  if (k > 0) return { get g() { return arguments.length + 10; } };
  return { get g() { return 7; } };
}
console.log(mk(1).g, mk(0).g);
const c = { get g() { return 3; } };
const d = { get g() { return arguments.length + 20; } };
console.log(c.g, d.g);
