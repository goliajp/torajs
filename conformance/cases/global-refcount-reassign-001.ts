// a top-level binding of every refcounted kind that a function reads
// (so it lives in a module slot) can be reassigned, null included
let m: Map<string, number> | null = new Map();
let st: Set<number> | null = new Set([1]);
let d: Date | null = new Date(0);
let r: RegExp | null = /a+/g;
let p: Promise<number> | null = Promise.resolve(1);
let b: bigint | null = 10n;
let wm: WeakMap<object, number> | null = new WeakMap();
function view(): string {
  return [m === null ? "-" : m.size, st === null ? "-" : st.size, d === null ? "-" : d.getTime(),
    r === null ? "-" : r.source, p === null ? "-" : "p", b === null ? "-" : String(b), wm === null ? "-" : "w"].join(" ");
}
console.log(view());
for (let i = 0; i < 1000; i++) {
  m = new Map([["k" + i, i]]);
  st = new Set([i, i + 1]);
  d = new Date(i);
  r = new RegExp("x" + (i % 3));
  b = BigInt(i);
}
console.log(view());
m = null; st = null; d = null; r = null; p = null; b = null; wm = null;
console.log(view());
m = new Map(); m.set("z", 26);
console.log(view());
