// A value written in several blocks and read in a block numbered
// BEFORE all of them. φ destruction builds exactly that: one ValueId
// gets a `copy` in every predecessor of the join, and the join can sit
// earlier in block order than its predecessors.
//
// The linear live interval then runs from the USE to the last DEF, so
// a call inside the join — before the use — falls outside it, and the
// allocator handed the value X0 across that call. `typeof x === "lit"`
// lowers to precisely this shape (a length test, a byte cascade, and a
// `__torajs_str_drop` in the join before the return).
const a: any[] = [1, 2, 3];

// `some` / `find` / `findIndex` break on a true predicate, `every` on
// a false one — all four read the callback's return through that slot.
console.log(a.some(v => typeof v === "zzz"));
console.log(a.every(v => typeof v === "zzz"));
console.log(a.find(v => typeof v === "undefined"));
console.log(a.findIndex(v => typeof v === "zzz"));

// The predicate has to actually run for every element: a return value
// misread as `true` stops `some` after the first one.
let calls = 0;
console.log(a.some(v => { calls = calls + 1; return typeof v === "zzz"; }), calls);

// Same slot shape reached through a local and through a named binding.
console.log(a.some(v => { const t = typeof v; return t === "zzz"; }));
const pred = (v: any): boolean => typeof v === "zzz";
console.log(a.some(pred));

// A hole must still be skipped, and a dense typed source must not pay
// for the gate — both answers are unchanged by the fix.
const h: any[] = [1, , 3];
console.log(h.some(v => v === undefined), h.filter(() => true).length);
