// T-30 — Date setters + annexB methods. ECMAScript §B.2.4 specifies
// the legacy `getYear` / `setYear` (year - 1900 / year + 1900 if 0-99)
// and `toGMTString` (alias for toUTCString). §21.4.4.27 specifies
// `setTime`. Pre-T-30 tora rejected calls to all four with "no
// member `.setX` on type Date" — multiple test262 annexB Date cases
// blocked.
//
// Implementation: runtime_date.c adds 4 helpers
// (__torajs_date_set_time / _set_year / _get_year / _to_gmt_string),
// reusing existing localtime_decompose + __torajs_date_components_to_local_ms
// for setYear's local-time recompose. ssa_lower's existing Date-method
// dispatch arm (the long matches! at line ~16261) extends to recognize
// the four new method names + routes to the new intrinsics. setTime /
// setYear take 1 arg (Number → I64); the others are arity-zero on Date.

let d = new Date(0);
console.log(d.getTime());        // 0

// setTime — overwrite ms slot, return the new ms.
const ms = d.setTime(86400000);
console.log(ms);                 // 86400000
console.log(d.getTime());        // 86400000

// setYear with year < 100 → +1900 (annexB compat shim).
let d2 = new Date(0);
console.log(d2.getYear());       // 70 (1970 - 1900)
d2.setYear(99);
console.log(d2.getYear());       // 99
console.log(d2.getFullYear());   // 1999

// setYear with year ≥ 100 → use as-is.
d2.setYear(2026);
console.log(d2.getYear());       // 126
console.log(d2.getFullYear());   // 2026

// toGMTString = toUTCString (annexB alias).
let d3 = new Date(0);
console.log(d3.toGMTString());   // Thu, 01 Jan 1970 00:00:00 GMT
console.log(d3.toUTCString());   // Thu, 01 Jan 1970 00:00:00 GMT
