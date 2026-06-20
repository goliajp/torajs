// S234 — `parseInt(s, undefined)` / `Number.parseInt(s, undefined)` per ES
// §19.2.5.1 / §21.1.2.13 step 2-3: ToInt32(undefined) = 0; step 8 then
// folds R==0 back to base 10 (or 16 if the input has a "0x"/"0X" prefix
// per step 11). The check.rs S234 widens the arg-1 type gate to
// Number|Undefined; ssa_lower substitutes ConstI64(0) for undef radix
// so the helper's auto-detect branch picks up the spec default.
console.log(parseInt("10", undefined));
console.log(parseInt("ff", undefined));
console.log(parseInt("0xff", undefined));
console.log(Number.parseInt("10", undefined));
console.log(Number.parseInt("ff", undefined));
console.log(Number.parseInt("0xff", undefined));
