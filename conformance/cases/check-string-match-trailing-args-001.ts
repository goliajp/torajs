// S286 — String.{match,matchAll}(re, ...trailing) trailing-arg ignore
// per ES §22.1.3.{11,13}. Spec reads only `re`; tora's regex helper
// ABI is (Str, RegExp) so trailing args typecheck-and-drop in
// check.rs and lower-and-drop in ssa_lower so step()-style side-
// effect exprs fire per ES eval-then-discard semantics.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const s: string = "hello world";

// match — no trailing (baseline)
const re1: RegExp = /[aeiou]/g;
const m1 = s.match(re1);
console.log("m1 length:", m1.length);
console.log("m1[0]:", m1[0]);
console.log("m1[1]:", m1[1]);
console.log("m1[2]:", m1[2]);

// match — single trailing
const re2: RegExp = /[aeiou]/g;
const m2 = s.match(re2, step("extra1"));
console.log("m2 length:", m2.length);
console.log("m2[0]:", m2[0]);
console.log("calls after m2:", calls);

// match — multi trailing
const re3: RegExp = /[aeiou]/g;
const m3 = s.match(re3, step("e1"), step(42), step(true));
console.log("m3 length:", m3.length);
console.log("m3[0]:", m3[0]);
console.log("calls after m3:", calls);

// matchAll — no trailing (baseline) — iterator-style for-of
const re4: RegExp = /[aeiou]/g;
const a4 = s.matchAll(re4);
let cnt4 = 0;
for (const m of a4) {
    cnt4 = cnt4 + 1;
    console.log("a4 m[0]:", m[0]);
}
console.log("a4 cnt:", cnt4);

// matchAll — single trailing
const re5: RegExp = /[aeiou]/g;
const a5 = s.matchAll(re5, step("trail"));
let cnt5 = 0;
for (const m of a5) {
    cnt5 = cnt5 + 1;
    console.log("a5 m[0]:", m[0]);
}
console.log("a5 cnt:", cnt5);
console.log("calls after a5:", calls);

// matchAll — multi trailing
const re6: RegExp = /[aeiou]/g;
const a6 = s.matchAll(re6, step("t1"), step(7), step(false));
let cnt6 = 0;
for (const m of a6) {
    cnt6 = cnt6 + 1;
    console.log("a6 m[0]:", m[0]);
}
console.log("a6 cnt:", cnt6);
console.log("calls final:", calls);
