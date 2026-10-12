// A store of a function value of another face into a fn-typed binding
// that has no initializer of its own: a parameter (plain, captured and
// written by a closure, of an arrow, of a generic function, of a class
// method, with a default), a `for-of` head, and a top-level binding
// written from inside a function. Every call through the binding after
// the store answers the stored value's own result.
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }

function h(f: (y?: number) => number) { f = mk2(1); return f() }
console.log(h((y?: number) => 1));

function h2(f: (y?: number) => number) { const r = f(); f = mk2(2); return r + f() + f(1) }
console.log(h2((y?: number) => 10));

function cap(f: (y?: number) => number) { const set = () => { f = mk2(3) }; set(); return f() + f(2) }
console.log(cap((y?: number) => 1));

const arrow = (f: (y?: number) => number) => { f = mk2(4); return f() + f(1) };
console.log(arrow((y?: number) => 1));

function gen<T>(x: T, f: (y?: number) => number) { f = mk2(5); return f() }
console.log(gen(1, (y?: number) => 1), gen("s", (y?: number) => 1));

class K { m(f: (y?: number) => number) { f = mk2(6); return f() } }
console.log(new K().m((y?: number) => 1));

function dflt(f: (y?: number) => number = (y?: number) => 2) { const a = f(); f = mk2(7); return a + f() }
console.log(dflt(), dflt((y?: number) => 3));

function loop(n: number, f: (y?: number) => number) {
  let s = 0;
  for (let i = 0; i < n; i++) { s += f(i); if (i == 1) f = mk2(8) }
  return s;
}
console.log(loop(4, (y?: number) => (y === undefined ? 0 : y)));

function heads(fs: ((y?: number) => number)[]) {
  let s = 0;
  for (let f of fs) { f = mk2(9); s += f() }
  return s;
}
console.log(heads([(y?: number) => 1, (y?: number) => 2]));
for (let f of [(y?: number) => 1]) { f = mk2(10); console.log(f(), f(1)) }

let g: (y?: number) => number = (y?: number) => 1;
function setG() { g = mk2(11) }
setG();
function useG() { return g() + g(2) }
console.log(g(), useG());
