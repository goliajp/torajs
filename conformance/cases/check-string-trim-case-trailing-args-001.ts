// S281 — String.{trim,trimStart,trimEnd,trimLeft,trimRight,toUpperCase,
// toLowerCase,toWellFormed,isWellFormed}(...trailing) trailing-arg ignore
// per ES §22.1.3.{28-34,10}. Spec-defined 0-arg methods; tora helpers
// are recv-only. Same S272 family — trailing args must eval-then-
// discard, not silent-drop.
//
// toLocale{Upper,Lower}Case excluded — they own the S140 sig + drop_args
// path which already silent-drops the locales arg.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const s = "  Foo  ";

console.log(s.trim());
console.log(s.trim(step(1)));
console.log(s.trim(step(2), step(3)));
console.log("calls after trim:", calls);

console.log(s.trimStart());
console.log(s.trimStart(step(4)));
console.log("calls after trimStart:", calls);

console.log(s.trimEnd());
console.log(s.trimEnd(step(5)));
console.log("calls after trimEnd:", calls);

// Legacy aliases
console.log(s.trimLeft());
console.log(s.trimLeft(step(6)));
console.log(s.trimRight());
console.log(s.trimRight(step(7)));
console.log("calls after trimLeft/trimRight:", calls);

console.log(s.toUpperCase());
console.log(s.toUpperCase(step(8), step(9)));
console.log("calls after toUpperCase:", calls);

console.log(s.toLowerCase());
console.log(s.toLowerCase(step(10)));
console.log("calls after toLowerCase:", calls);

// ES2024 WellFormed
console.log("abc".isWellFormed());
console.log("abc".isWellFormed(step(11)));
console.log("calls after isWellFormed:", calls);

console.log("abc".toWellFormed());
console.log("abc".toWellFormed(step(12)));
console.log("calls after toWellFormed:", calls);
