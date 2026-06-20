// S284 — String.{slice,substring,substr,padStart,padEnd}(a, b,
// ...trailing) trailing-arg ignore per ES §22.1.3.{20,22,23,16,17}.
// Spec-defined 2-arg methods; tora helpers are 2-arg only. Same S272
// family — trailing args must eval-then-discard, not silent-drop.
// Widens from S241's `args.len() == 3` (single trailing) to any count.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const s = "0123456789";

// slice
console.log(s.slice(2, 5));
console.log(s.slice(2, 5, step(1)));
console.log(s.slice(2, 5, step(2), step(3)));
console.log("calls after slice:", calls);

// substring
console.log(s.substring(2, 5));
console.log(s.substring(2, 5, step(4), step(5)));
console.log("calls after substring:", calls);

// substr
console.log(s.substr(2, 3));
console.log(s.substr(2, 3, step(6), step(7)));
console.log("calls after substr:", calls);

// padStart
console.log("a".padStart(5, "-"));
console.log("a".padStart(5, "-", step(8)));
console.log("a".padStart(5, "-", step(9), step(10)));
console.log("calls after padStart:", calls);

// padEnd
console.log("a".padEnd(5, "-"));
console.log("a".padEnd(5, "-", step(11), step(12)));
console.log("calls after padEnd:", calls);
