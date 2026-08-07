// rotation 325 — Object.getOwnPropertyDescriptor over an owned
// receiver temp (the `Error.prototype` member-read chain answers an
// owned Any box). The lane released its key but never its receiver;
// the stranded +1 sat on %Error.prototype% through the at-exit cycle
// drain and cut the error-prototype reference cycle in two (the
// detector saw 8 of 10 cells judged WHITE and three post-free decs).
// Values here pin bun parity; the rc balance is what the underflow
// census checks.
const d1: any = Object.getOwnPropertyDescriptor(Error.prototype, "name");
console.log(d1.value + "|" + d1.writable + "|" + d1.enumerable + "|" + d1.configurable);
const d2: any = Object.getOwnPropertyDescriptor(TypeError.prototype, "message");
console.log(d2.value === "" ? "empty" : d2.value, d2.writable);
const d3: any = Object.getOwnPropertyDescriptor(Error.prototype, "toString");
console.log(typeof d3.value, d3.enumerable);
const missing: any = Object.getOwnPropertyDescriptor(Error.prototype, "nope");
console.log(missing === undefined);
