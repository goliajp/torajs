// P4.prototype-chain — `Object.getPrototypeOf(instance)` returns
// the class's prototype object, and `Object.getPrototypeOf(C.prototype)`
// returns the parent class's prototype (or null for the root). Pre-fix
// tora stubs getPrototypeOf to always return null (nominal class
// system has no real prototype objects). Spec §10.1.1 / §19.1.2.13.
//
// Minimum P4.prototype-chain ship: prototype singletons exposed +
// [[Prototype]] slot on instances + getPrototypeOf reads from slot.
// Method resolve via nominal vtable stays for perf; this is the
// readback / introspection path that test262 + tools use.

class A {
  ax(): number { return 1; }
}
class B extends A {
  bx(): number { return 2; }
}
class C extends B {
  cx(): number { return 3; }
}

const c = new C();

// 1. instance's proto = its class's prototype
const p1: any = Object.getPrototypeOf(c);
console.log(p1 !== null);   // true
console.log(p1 === C.prototype);  // true

// 2. C.prototype's proto = B.prototype
const p2: any = Object.getPrototypeOf(C.prototype);
console.log(p2 === B.prototype);  // true

// 3. B.prototype's proto = A.prototype
const p3: any = Object.getPrototypeOf(B.prototype);
console.log(p3 === A.prototype);  // true

// 4. A.prototype's proto = Object.prototype (or null in subset; root)
const p4: any = Object.getPrototypeOf(A.prototype);
console.log(p4 !== undefined);  // true (either Object.prototype or null both ok)
