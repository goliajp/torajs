// a top-level binding read off a variadic closure field of a
// top-level literal is called with any arity, from named fns too
let base = 100;
let src = { f: (...xs: number[]) => xs.length, s: (a: number, ...xs: number[]) => a + xs.length + base, n: 5 };
const g = src.f;
const h = g;
const s = src.s;
const typed: (...xs: number[]) => number = src.f;
function direct() { return g(1, 2, 3) + " " + h() + " " + typed(1) + " " + s(1, 2, 3) }
function caught() { const t = () => h(1, 2); return t() }
function captured() { base = 1000; return s(0) }
console.log(direct(), caught(), captured(), g(4, 5), src.n);
