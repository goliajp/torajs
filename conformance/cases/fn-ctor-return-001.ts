// RFC 20260726-new-on-function blade 3 — spec §10.2.2 step 8: a
// constructor's own return value wins over the fresh receiver, but only
// when it is an Object.

// An object return replaces the receiver entirely.
function A(x: number) { this.x = x; return { y: 2 }; }
console.log(new A(1).y);

// A primitive return is ignored — the receiver survives.
function B(x: number) { this.x = x; return 7; }
console.log(new B(5).x);

// `typeof null` is "object", so null needs excluding explicitly or it
// would win here and the receiver would be lost.
function C(x: number) { this.x = x; return null; }
console.log(new C(6).x);

// A bare `return;` yields undefined, which is not an Object.
function D(x: number) { this.x = x; if (x < 0) { return; } }
console.log(new D(3).x);

// Returning conditionally: both paths have to be right at runtime,
// since which one runs is not known when the factory is built.
function E(x: number) { this.x = x; if (x > 100) { return { z: 1 }; } }
console.log(new E(1).x);
const e2: any = new E(200);
console.log(e2.z);

// Returning the receiver itself is the identity case.
function F(x: number) { this.x = x; return this; }
console.log(new F(9).x);

// A returned object built from the arguments, the common factory shape.
function G(a: number, b: number) { this.sum = a + b; return { sum: a * b }; }
console.log(new G(3, 4).sum);
