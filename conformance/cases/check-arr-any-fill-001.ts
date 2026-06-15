// `Array<Any>.fill(v)` pre-fix took two hits: check.rs rejected
// any cross-type fill value with "must match elem type Any", and
// even after the typecheck escape the ssa-lower non-Copy per-slot
// loop wrote raw bits through StoreDyn without NaN-boxing — the
// 8-byte AnyValue slot decoded as garbage on the next walk and
// SIGSEGV'd in the heap drop. Fix routes through new runtime
// helper __torajs_arr_fill_any (NaN-box pack + per-slot drop +
// rc_inc) so the SSA emit stays a single Call.

const a: any[] = [0, 0, 0, 0];
a.fill(1);
console.log(a.join(","));
a.fill("x");
console.log(a.join(","));
a.fill(true);
console.log(a.join(","));
a.fill(null);
console.log(a.join(","));
a.fill(2.5);
console.log(a.join(","));

const b: any[] = [10, 20, 30, 40, 50];
b.fill(0, 1, 4);
console.log(b.join(","));
b.fill("y", 2);
console.log(b.join(","));

const c: any[] = new Array<any>(3).fill(7);
console.log(c.join(","));

const d: any[] = new Array<any>(4).fill("z");
console.log(d.join(","));

// negative start/end (relative-to-len)
const e: any[] = [1, 2, 3, 4, 5];
e.fill(9, -2);
console.log(e.join(","));
