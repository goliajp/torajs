// S268 — Date instance setter trailing-arg ignore per ES
// §21.4.4.{20-26} (per-field setters take up to N args; the
// rest silent-drop per ES trailing-arg ignore). tora's fixed-
// max-arity sigs in check.rs rejected the next arg; SSA-emit's
// `for i in 0..target_arity` loop already takes only args
// [0..arity], so the runtime call shape stays correct — S268
// adds checktime widen + ssa-emit skip(arity) eval-and-drop
// to preserve side effects.

const d = new Date(2020, 0, 1);
console.log(d.setFullYear(2021, 6, 15, 99));
console.log(d.setFullYear(2022, 0, 1, 88, 77));
console.log(d.setMonth(3, 99, 66));
console.log(d.setMonth(5, 1, 88, 77));
console.log(d.setDate(10, 99));
console.log(d.setHours(12, 0, 0, 0, 99));
console.log(d.setHours(13, 1, 2, 3, 4, 5, 6));
console.log(d.setMinutes(30, 45, 0, 99));
console.log(d.setSeconds(45, 0, 88));
console.log(d.setMilliseconds(500, 99));
console.log(d.setTime(1234567890, 99));
