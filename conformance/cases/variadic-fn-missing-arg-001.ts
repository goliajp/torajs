// A function stored behind a variadic fn type and called with fewer
// arguments than it declares sees `undefined` in each parameter no
// argument reaches, whatever its own parameter types say.

const fs: Array<(...xs: number[]) => number> = [];
fs.push((a, b) => a + b);
fs.push((a: number, b: number) => a * 2 + b);
console.log(fs[0](1), fs[1](1));
console.log(fs[0](1, 2), fs[1](1, 2));

const r: number = fs[0](4);
console.log(Number.isNaN(r));

function call1(h: (...xs: number[]) => number): number {
  return h(4);
}
console.log(call1((a, b) => a + b));
console.log(call1((a) => a + 1));

const gs: Array<(...xs: number[]) => string> = [];
gs.push((a: number, b: number) => typeof b + " " + (b === undefined) + " " + (a + b));
console.log(gs[0](1));
console.log(gs[0](1, 2.5));

const o: { f: (...xs: number[]) => number } = { f: (a: number, b: number) => a + b };
console.log(o.f(1), o.f(1, 2));

let w: (...xs: number[]) => number = (a: number) => a;
w = (a: number, b: number) => a + b;
console.log(w(1), w(1, 2));

const ks: ((...xs: number[]) => number)[] = [(a: number, b: number) => a - b];
const args: number[] = [5];
console.log(ks[0](...args), ks[0](5, 1));

const ss: Array<(...xs: string[]) => string> = [];
ss.push((a, b) => a + b);
console.log(ss[0]("x"), ss[0]("x", "y"));
