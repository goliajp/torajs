// a function whose fn-typed return already has the closure repr, by
// returning an arrow or a slot read, handed to a parameter typed as a
// function returning a function: calls through the parameter get the
// fn object back, and the other functions handed there join it
type F = (n: number) => number;
const p: { g: F } = { g: (n) => n + 1 };
const objs: any = { f: (n: number) => n - 1 };
function dbl(n: number): number { return n * 2; }
function mk(k: number): F { return (n) => n + k; }
function pk(k: number): F { return dbl; }
function slot(k: number): F { if (k > 0) return dbl; const v = p.g; return v; }
function direct(k: number): F { if (k > 0) return dbl; return objs.f; }
function apply(f: (k: number) => F, k: number): number { return f(k)(5); }
console.log(mk(1)(5), slot(1)(5), slot(0)(5), direct(0)(5));
console.log(apply(mk, 2), apply(pk, 1), apply(slot, 1), apply(slot, 0), apply(direct, 0));
let t = 0;
for (let i = 0; i < 10; i++) t += apply(i % 2 ? mk : pk, i);
console.log(t);
