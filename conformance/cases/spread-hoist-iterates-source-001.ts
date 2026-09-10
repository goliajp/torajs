// A spread whose source is not a plain identifier (`f(...mk())`) is
// hoisted into a temp so the fixed-arity expansion can read it by
// index. The temp used to hold the source ITSELF — right for an
// array, wrong for every other iterable: a generator was never run
// (its body, its throw) and every slot read undefined. The temp now
// holds `[...src]`, one §7.4 walk in the source's own position.
function f(a: number, b: number) { return [a, b]; }
function* g(): Generator<number> { console.log("g ran"); yield 1; yield 2; }
console.log(f(...g()));
function* gt(): Generator<number> { yield 7; throw new Error("gen"); }
try { console.log(f(...gt())); } catch (e: any) { console.log("caught", e.message); }
function mk(): number[] { return [3, 4]; }
console.log(f(...mk()), f(...mk().map((x) => x * 10)));
function three(a: number, b: number, c: number) { return a + b + c; }
console.log(three(...g(), 100));
const s = new Set([5, 6]);
console.log(f(...(s as any)), Math.max(...g()));
const xs: number[] = [];
xs.push(...g());
console.log(xs);
function h(a: string, b: string) { return a + b; }
console.log(h(..."ab"));
