// P5.6 follow-up — Array<Any> spread via the new
// `__torajs_arr_extend_any` runtime helper. Previously the bare
// `[...anyArr]` shape SIGSEGV'd because the 8-byte-stride
// arr_extend_unchecked walked the 16-byte tagged-slot layout
// misaligned; now lower_array_any_literal routes through the
// tagged-slot extender which understands the (tag, value) pair
// layout and rc-bumps heap children as they get shared into the
// destination.

const a: any[] = [1, "hi", true];
const b: any[] = [...a, 99, "end"];
console.log(b.length);   // 5
for (const v of b) { console.log(v); }

// Multiple spreads + tail literal.
const c: any[] = [...a, ...a, 42];
console.log(c.length);   // 7
console.log(c[0]);       // 1
console.log(c[2]);       // true
console.log(c[3]);       // 1
console.log(c[6]);       // 42

// Empty spread.
const empty: any[] = [];
const d: any[] = [...empty, "only"];
console.log(d.length);   // 1
console.log(d[0]);       // only
