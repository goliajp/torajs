// S289 — Array<T>.flat(depth, ...trailing) trailing-arg ignore per
// ES §23.1.3.10. Spec reads only `depth`; tora's runtime helper +
// SSA-emit also peek only args[0]. Trailing args typecheck-and-
// drop + lower-and-drop so step() fires per ES eval-then-discard
// semantics.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const xs: number[][] = [[1, 2], [3, 4], [5]];

console.log("flat() bare:");
console.log(xs.flat());

console.log("flat(1) bare:");
console.log(xs.flat(1));

console.log("flat(1, trailing):");
console.log(xs.flat(1, step("extra")));
console.log("calls:", calls);

console.log("flat(2, multi trailing):");
console.log(xs.flat(2, step("e1"), step(42), step(true)));
console.log("calls:", calls);

// flat(0) returns a shallow clone whose nested-array pretty-print
// differs across bun/tora (multi-line vs single-line); probe via
// .length to dodge the formatting gap while still proving trailing
// fires.
const flat0: number[][] = xs.flat(0, step("e2"));
console.log("flat(0) length:", flat0.length);
console.log("flat(0)[0][0]:", flat0[0][0]);
console.log("calls:", calls);

const ys: number[][][] = [[[1], [2]], [[3]]];
console.log("nested flat(2, trail):");
console.log(ys.flat(2, step("nt")));
console.log("calls final:", calls);
