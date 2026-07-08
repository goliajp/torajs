// chunk 705 — `callee?.(args)` optional call (ES2020 §13.3.9):
// nullish callee short-circuits to undefined without evaluating the
// args; plain callee delegates to the regular call path; a
// non-callable non-nullish callee is a catchable TypeError.
const g = (x: number) => x * 2;
console.log(g?.(21));
function h(x: number): number { return x + 5; }
console.log(h?.(10));
const f: any = null;
console.log(f?.());
const u: any = undefined;
console.log(u?.(5));
const ga: any = (x: number) => x * 3;
console.log(ga?.(7));
const o: any = { m: (x: number) => x * 10 };
console.log(o.m?.(2));
console.log(o.nope?.());
console.log(o?.m?.(3));
const nc: any = 42;
try { nc?.(); } catch (e) { console.log("caught non-callable"); }
let effects = 0;
const bump = () => { effects++; return 7; };
const nil: any = null;
console.log(nil?.(bump()), "effects", effects);
console.log(o.m?.(bump()), "effects", effects);
console.log(typeof g?.(1));
console.log(typeof nil?.(1));
