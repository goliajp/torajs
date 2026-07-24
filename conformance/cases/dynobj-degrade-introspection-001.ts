// dynobj-degrade introspection triggers (rotation 203 chunk 3) — a
// binding used as the receiver of the descriptor / proto
// introspection family (Object.getOwnPropertyDescriptor /
// getOwnPropertyNames / getPrototypeOf / ...) degrades to the dynobj
// lane: the struct lane silently mis-answers these (the r205
// pass→bug collateral shapes, previously masked by the name-keyed
// collector's collision degrade).

let a = { x: 1 };
const d = Object.getOwnPropertyDescriptor(a, "x");
console.log(d.value);
console.log(d.writable);

let p = { y: 2 };
console.log(Object.getPrototypeOf(p) === Object.prototype);

let n = { z: 3 };
const names = Object.getOwnPropertyNames(n);
console.log(names.length);
console.log(names[0]);
