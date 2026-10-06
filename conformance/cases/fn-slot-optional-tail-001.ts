// a function value with optional parameters past the slot's arity is
// called through the narrower function type: the parameters the call
// does not pass bind `undefined`

interface P { v: number }
class C { constructor(public w: number) {} }
const a1: (n: number) => number = (n, p?: P) => { return p === undefined ? n : n + p.v; };
console.log(a1(9));
const f1: (n: number) => number = function (n, p?: P) { return p === undefined ? n : n + p.v; };
console.log(f1(9));
const f2: (n: number) => number = function (n: number, p?: P): number { return p === undefined ? n + 1 : n + p.v; };
console.log(f2(9));
function wrap() {
  const f3: (n: number) => number = function (n, p?: P) { return p === undefined ? n + 2 : n + p.v; };
  console.log(f3(9));
  const base = 100;
  const f4: (n: number) => number = function (n, p?: P) { return p === undefined ? n + base : n + p.v; };
  console.log(f4(9));
}
wrap();
const f5: (n: number) => string = function (n, s?: string) { return s === undefined ? "u" + n : s; };
console.log(f5(1));
const f6: (n: number) => number = function (n, m?: number) { return m === undefined ? -n : m; };
console.log(f6(4));
const b1: (n: number) => string = function (n, b?: boolean) { return b === undefined ? "nob" : String(b); };
console.log(b1(1));
const xs1: (n: number) => number = (n, xs?: number[]) => xs === undefined ? -1 : xs.length;
console.log(xs1(1));
const c1: (n: number) => number = function (n, c?: C) { return c === undefined ? n : c.w; };
console.log(c1(5));
function top(n: number, p?: P): number { return p === undefined ? n * 10 : p.v; }
const t1: (n: number) => number = top;
console.log(t1(3));
function host(cb: (n: number) => number) { return cb(2) + cb(4); }
console.log(host(top));
console.log(host(function (n, p?: P) { return p === undefined ? n : p.v; }));
console.log(host((n: number, c?: C) => c === undefined ? n + 1 : c.w));
const many: (n: number) => string = (n, p?: P, s?: string, k?: number) =>
  [n, p === undefined, s === undefined, k === undefined].join(" ");
console.log(many(0));
const args1: (n: number) => number = function (n, p?: P) { return arguments.length; };
console.log(args1(1));
const loose: (a: number) => any = (a, b?) => b === undefined ? "none" : b;
console.log(loose(1));
const g: (a: number) => string = (a: number, b?: string) => b ?? "dflt";
console.log(g(2));
