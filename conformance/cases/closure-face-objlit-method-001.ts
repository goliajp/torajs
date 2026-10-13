// An object literal's method whose fn-typed parameter a later call hands
// a function value of another face: the body is checked with the literal,
// before that call, and a binding initialized by a call of the parameter
// still answers what the value returns.
function mkS(k: number): (y?: number) => any { return (y?: number) => "s" + k }
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }

const obj = {
  m(f: (y?: number) => number) { const v = f(); return typeof v + ":" + v },
  n(f: (y?: number) => number) { const v = f(2); return v },
  t(f: (y?: number) => number) { const a = f(1), b = f(); return a + "|" + b },
};
console.log(obj.m(mkS(1)), obj.m(mk2(1)), obj.m((y?: number) => 4));
const r = obj.n(mkS(2));
console.log(typeof r + ":" + r, obj.n(mk2(2)));
console.log(obj.t(mkS(3)), obj.t(mk2(3)));

function inner() {
  const o2 = { m(f: (y?: number) => number) { const v = f(); return typeof v + ":" + v } };
  return o2.m(mkS(5)) + " " + o2.m(mk2(5));
}
console.log(inner());

// a literal whose method only ever takes values of its own face keeps the
// number its parameter returns
const plain = { m(f: (y?: number) => number) { const v = f(); return v * 2 } };
console.log(plain.m((y?: number) => 21), plain.m(() => 4));

class K { run(f: (y?: number) => number) { const v = f(); return typeof v + ":" + v } }
console.log(new K().run(mkS(6)), new K().run(mk2(6)));

// a closure a function-local binding holds is checked before its call too
function local() {
  const g = (f: (y?: number) => number) => { const v = f(); return typeof v + ":" + v };
  const h = (f: (y?: number) => number) => f() + f(1);
  return g(mkS(7)) + " " + h(mkS(8)) + " " + g(mk2(7)) + " " + h(mk2(8));
}
console.log(local());

// a call through a parameter checked before the store of another face
function st(f: (y?: number) => number) { const a = f(); f = mk2(9); return a + f() }
console.log(st((y?: number) => 1));
