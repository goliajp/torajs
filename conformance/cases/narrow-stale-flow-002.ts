// every shape of call hands the value on: a method, a function value,
// an optional call, spread arguments, a callback; and the store that
// undoes the narrow can run without a call in the source: an accessor,
// a conversion method, an iterator step
type Node = { n: number };
let q: Node | null = { n: 1 };
function f(): boolean { q = null; return true }
function g(x: Node): number { return x.n }
function reset(): void { q = { n: 1 } }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  reset();
}

class O { m(x: Node): number { return x.n } }
const o = new O();
run("method", () => { if (q !== null && f()) console.log(o.m(q)) });

const k = (x: Node): number => x.n;
run("value", () => { if (q !== null && f()) console.log(k(q)) });

const gg: ((x: Node) => number) | undefined = g;
run("optcall", () => { if (q !== null && f()) console.log(gg?.(q)) });

function rest(...xs: Node[]): number { return xs[0].n }
run("spread", () => { if (q !== null && f()) console.log(rest(...[q])) });

function apply(cb: (x: Node) => number, v: Node): number { return cb(v) }
run("callback", () => { if (q !== null && f()) console.log(apply((x: Node): number => x.n, q)) });

class C { get x(): number { q = null; return 1 } }
const c = new C();
run("getter", () => { if (q !== null && c.x > 0) console.log(g(q)) });

const lit = { get w(): number { q = null; return 1 } };
run("literal getter", () => { if (q !== null && lit.w > 0) console.log(g(q)) });

class S { set x(v: number) { q = null } }
const s = new S();
run("setter", () => { if (q !== null) { s.x = 1; console.log(g(q)) } });

class T { toString(): string { q = null; return "t" } }
const t = new T();
run("template", () => { if (q !== null) { console.log(`${t}`); console.log(g(q)) } });
run("String", () => { if (q !== null) { console.log(String(t)); console.log(g(q)) } });

class V { valueOf(): number { q = null; return 1 } }
const v = new V();
run("valueOf", () => { if (q !== null) { console.log((v as any) + 1); console.log(g(q)) } });

run("stringify", () => { if (q !== null) { console.log(JSON.stringify(lit)); console.log(g(q)) } });

function* gen(): Generator<number> { q = null; yield 1 }
run("generator", () => { if (q !== null) { for (const e of gen()) console.log(e); console.log(g(q)) } });

class It {
  i = 0;
  next(): IteratorResult<number> { q = null; this.i++; return { value: this.i, done: this.i > 1 } }
  [Symbol.iterator](): It { return this }
}
run("iterator", () => { if (q !== null) { for (const e of new It()) console.log(e); console.log(g(q)) } });

// a call made on the previous turn of a loop
run("loop", () => { if (q !== null) { for (let i = 0; i < 2; i++) { console.log(g(q)); f() } } });

// the call ran before the test
run("before", () => { if (f() && q === null) { reset(); if (q !== null) console.log(g(q)) } });
