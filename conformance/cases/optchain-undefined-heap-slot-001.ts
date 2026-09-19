// Optional chains short-circuit on a T | undefined heap slot holding
// undefined (the generic undefined cell), not only on null.
class C { f = "field"; m() { return "C.m:" + this.f; } }
let a: C | undefined = undefined;
let arr: number[] | undefined = undefined;
let s: string | undefined = undefined;
let fn: (() => number) | undefined = undefined;
if (Math.random() > 2) { a = new C(); arr = [1]; s = "x"; fn = () => 1; }
console.log(a?.m(), a?.f, a?.["f"], arr?.[0], arr?.length, s?.length, fn?.());
a = new C(); arr = [7]; s = "xyz"; fn = () => 5;
console.log(a?.m(), a?.f, a?.["f"], arr?.[0], arr?.length, s?.length, fn?.());
const n: C | null = null;
console.log(n?.m(), n?.f);
