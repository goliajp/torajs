// chunk 709 — builtin methods through `?.()` on any receivers:
// GetV-existence probe (dynobj/struct exact, builtin optimistic) +
// opt method-call dispatch (no-such answers undefined, resolved
// non-callables still throw). closes the 705 recorded face.
const s: any = "hello";
console.log(s.toUpperCase?.());
console.log(s.nope?.());
console.log(s.slice?.(1, 3));
const n: any = 42;
console.log(n.toFixed?.(1));
console.log(n.slice?.(1));
const arr: any = [3, 1, 2];
console.log(arr.join?.("-"));
console.log(arr.nothere?.());
arr.push?.(9);
console.log(arr.join?.(","));
const o: any = { m: (x: number) => x * 10 };
console.log(o.m?.(2));
console.log(o.nope?.());
let effects = 0;
const bump = () => { effects++; return 1; };
console.log(o.nope?.(bump()), "effects", effects);
const x: any = { v: 42 };
try { x.v?.(); } catch (e) { console.log("caught non-callable"); }
const nil: any = null;
try { nil.m?.(); } catch (e) { console.log("caught nullish member read"); }
console.log(s.toUpperCase());
