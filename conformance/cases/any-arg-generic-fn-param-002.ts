// an `any` argument for a fn-typed parameter of a generic function:
// positions, explicit and inferred type arguments, methods and arrows
function first<T>(cb: (t: T) => T, x: T): T { return cb(x); }
function gen<T>(x: T, cb: (t: T) => T): T { return cb(x); }
function make<U>(cb: (n: number) => U): U { return cb(4); }
function both<A, B>(f: (a: A) => B, a: A, g: (b: B) => B): B { return g(f(a)); }
const objs: any = {
  arrow: (n: number) => n * 3,
  m(n: number) { return n + 100; },
  str: (s: string) => s.toUpperCase(),
  wide: (n: number) => "w" + n,
};
const a = objs.arrow;
const m = objs.m;
const s = objs.str;
console.log(first<number>(a, 2), first(a, 3), gen<number>(4, a), gen(5, a));
console.log(first(m, 1), gen(2, m), make(m), make(a));
console.log(first<string>(s, "ab"), gen("cd", s));
console.log(both(a, 2, m), both<number, number>(m, 1, a));
const w = objs.wide;
console.log(make(w), first<number>(w, 7));
console.log(first<number>(objs.arrow, 6), make(objs.m));
function take(h: any): number { return first<number>(h, 9); }
console.log(take(a), take(m));
// a typed callback still runs the typed instantiation
const dbl = (n: number): number => n * 2;
function sq(n: number): number { return n * n; }
console.log(first(dbl, 8), first(sq, 8), gen(3, sq));
// a non-callable behind `any` throws where it is called
const notFn: any = 42;
try { first<number>(notFn, 1); } catch (e) { console.log(e instanceof TypeError); }
let called = 0;
function maybeCall<T>(cb: (t: T) => T, x: T, go: boolean): T { if (go) { called++; return cb(x); } return x; }
console.log(maybeCall<number>(notFn, 5, false), called);
// the any argument reaches the generic callee from inside another
// generic body
function relay<T>(h: any, x: T): T { return first<T>(h, x); }
console.log(relay(a, 7), relay<number>(m, 8));
