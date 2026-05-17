// P4.prototype-chain Phase A — class name reachable as a value.
// Spec §15.7 class declarations introduce both a binding for the
// class type and a constructor function object visible at runtime
// under the class name. Pre-fix tora rejected `const x = A` at
// typecheck because `A` was type-only — value-position references
// returned "unknown identifier".
//
// First-class class objects are the substrate foundation for the
// rest of P4 (prototype chain readback, extends-chain, Function.
// prototype.bind, etc.). MVP scope here: expose the class name as a
// dynobj-backed Any value with at least a `name` property. Full
// constructor [[Call]] / [[Construct]] semantics are follow-ups.

class A {
  ax(): number { return 1; }
}

class B extends A {
  bx(): number { return 2; }
}

// 1. Class name as value
const a_class: any = A;
console.log(a_class !== undefined);  // true
console.log(a_class !== null);       // true

// 2. .name property
console.log(a_class.name);   // "A"
const b_class: any = B;
console.log(b_class.name);   // "B"

// 3. Identity — same class twice gives same object (singleton)
console.log(A === A);  // true

// 4. Instance creation still works (no regression on `new`)
const a = new A();
console.log(a.ax());  // 1
const b = new B();
console.log(b.ax());  // 1 (inherited)
console.log(b.bx());  // 2
