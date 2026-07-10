// RFC 20260710-optional-undefined-repr C5 — member-path truthiness
// narrow: `if (o.cb)` narrows the Nullable fn field to callable in
// the guarded branch (eq-guard and negated forms too), and an
// optional fn field is Closure-repr so a lifted arrow / named fn
// stored in it dispatches correctly.
type O = { cb?: (n: number) => number; tag?: string };
const o: O = { cb: (n: number) => n * 3, tag: "x" };
if (o.cb) { console.log(o.cb(7)); }
if (!o.cb) { console.log("none"); } else { console.log(o.cb(10)); }
if (o.cb !== undefined) { console.log(o.cb(2)); }

const p: O = { cb: undefined, tag: undefined };
if (p.cb) { console.log(p.cb(1)); } else { console.log("no cb"); }
if (p.cb === undefined) { console.log("cb absent"); } else { console.log(p.cb(2)); }

if (o.tag) { console.log(o.tag.length); }

function triple(n: number): number { return n * 3; }
type F = { fn?: (n: number) => number };
const q: F = { fn: triple };
if (q.fn) { console.log(q.fn(5)); }
const r: F = {};
if (r.fn) { console.log(r.fn(9)); } else { console.log("r empty"); }
