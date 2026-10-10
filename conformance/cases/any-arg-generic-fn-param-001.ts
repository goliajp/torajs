// an `any` argument for a fn-typed parameter of a generic function
function gen<T>(x: T, cb: (t: T) => T): T { return cb(x); }
function first<T>(cb: (t: T) => T, x: T): T { return cb(x); }
function make<U>(cb: (n: number) => U): U { return cb(4); }
const objs: any = { f: (n: number) => n + 1, s: (s: string) => s + "!", k: (n: number) => "k" + n };
const v = objs.f;
console.log(gen(2, v));
console.log(first(v, 10));
console.log(gen("a", objs.s));
console.log(make(objs.k));
const parsed = JSON.parse('{"n": 5}');
console.log(gen(parsed.n, v));
