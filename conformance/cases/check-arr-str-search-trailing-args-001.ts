// S278 — Array.{indexOf,lastIndexOf,includes} + String.{indexOf,
// lastIndexOf,includes,startsWith,endsWith}(needle, fromIndex,
// ...trailing) trailing-arg ignore per ES §22.1.3.{8,10,5,21,7} /
// §23.1.3.{14,16,17}. Spec reads only first 2 args; tora silent-
// dropped extras. S278 widens check.rs gate to `args.len() >= 3` +
// typecheck-and-drop args[2..]; ssa_lower mirror lowers trailing
// for side effects.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

// Array path — number[]
const xs: number[] = [1, 2, 3, 4, 5, 3];

console.log(xs.indexOf(3, 0, step(99), step(100)));
console.log("calls after arr indexOf:", calls);

console.log(xs.lastIndexOf(3, 4, step(50), step(51)));
console.log("calls after arr lastIndexOf:", calls);

console.log(xs.includes(3, 3, step(7), step(8)));
console.log("calls after arr includes:", calls);

// String path — same family
const s: string = "abcdef abc";

console.log(s.indexOf("abc", 0, step(11), step(12)));
console.log("calls after str indexOf:", calls);

console.log(s.lastIndexOf("abc", 9, step(13), step(14)));
console.log("calls after str lastIndexOf:", calls);

console.log(s.includes("abc", 5, step(15), step(16)));
console.log("calls after str includes:", calls);

console.log(s.startsWith("abc", 0, step(17), step(18)));
console.log("calls after str startsWith:", calls);

console.log(s.endsWith("abc", 10, step(19), step(20)));
console.log("calls after str endsWith:", calls);
