// §13.3.8.1 — a spread iterates its source (§7.4). The static
// expanders read an Ident source by INDEX, which is only what a
// spread means for an array: a generator object, a Set, a Map, a
// string, an untyped param bound to a Set — expanded by index, every
// slot read undefined and a generator body never ran (nor threw).
// Only an Ident whose declaration says "array" (annotation, array
// literal, rest param) is index-expanded now; everything else is
// hoisted into `[...src]` or wrapped onto the runtime spread lane,
// which iterates. The last two shapes are the ones the hoist cannot
// reach (an effectful prefix arg; a site inside `try`) — those ride
// the wrap.
function f(a: any, b: any) { return [a, b]; }
function f3(a: any, b: any, c: any) { return [a, b, c]; }
function* gen() { yield 1; yield 2; }
const g = gen();
console.log(f(...g));
const s = new Set([7, 8]);
console.log(f(...s));
const str = "ab";
console.log(f(...str));
const m = new Map([[1, "x"], [2, "y"]]);
console.log(f(...m));
function mk() { return [3, 4]; }
const ys = mk();
console.log(f(...ys));
function h(xs) { return f(...xs); }
console.log(h([3, 4]), h(new Set([5, 6])));
function h2(xs: number[]) { return f(...xs); }
console.log(h2([3, 4]));
const g2 = gen();
console.log(f3(0, ...g2));
const out: number[] = [1];
const g3 = gen();
out.push(...g3);
console.log(out);
const g4 = gen();
console.log(Math.max(...g4));
function dflt(a: number, b: number, c = 10) { return a + b + c; }
const zs = mk();
console.log(dflt(...zs));
let k = 0;
function side() { k++; return k; }
const g5 = gen();
console.log(f(side(), ...g5), k);
function* bad() { yield 1; throw new Error("boom"); }
const b = bad();
try { console.log(f(...b)); } catch (e) { console.log("caught", (e as Error).message); }
