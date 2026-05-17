// P4.3 extends-chain — multi-level inheritance: method resolve via
// nominal vtable (perf path) + prototype chain walk via repeated
// Object.getPrototypeOf calls + multi-level instanceof. Pre-fix
// `Object.getPrototypeOf` consumed its argument (consume_if_ident +
// emit_drop_value combo from the V3-18 stub era), which caused a
// silent use-after-free on the second call to getPrototypeOf with
// the same binding — the intercept dec'd the slot's value mid-scope
// while marking the binding moved, and subsequent reads returned a
// dangling pointer. Fix: getPrototypeOf is borrow-semantics; leave
// Ident args alone, only drop non-Ident temps.
//
// Acceptance: method resolve through 3-level chain works; identity
// across repeated getPrototypeOf calls on the same binding holds;
// chain walk via getPrototypeOf reaches each prototype in order.

class A {
  ax(): number { return 10; }
  shared(): string { return "from-A"; }
}
class B extends A {
  bx(): number { return 20; }
}
class C extends B {
  cx(): number { return 30; }
}

const c = new C();

// 1. Multi-level method resolve via nominal dispatch.
console.log(c.ax());     // 10 (inherited from A, 2 levels deep)
console.log(c.bx());     // 20 (inherited from B, 1 level)
console.log(c.cx());     // 30 (own)
console.log(c.shared()); // "from-A"

// 2. instanceof walks the full chain.
console.log(c instanceof A);   // true
console.log(c instanceof B);   // true
console.log(c instanceof C);   // true

// 3. Repeated getPrototypeOf on the same binding stays consistent —
//    no UAF, no identity drift. Pre-fix would crash or return false.
const p_first: any = Object.getPrototypeOf(c);
const p_second: any = Object.getPrototypeOf(c);
console.log(p_first === p_second);       // true
console.log(p_first === C.prototype);    // true

// 4. Multi-step chain walk: c → C.prototype → B.prototype → A.prototype.
const pC: any = Object.getPrototypeOf(c);
const pB: any = Object.getPrototypeOf(pC);
const pA: any = Object.getPrototypeOf(pB);
console.log(pC === C.prototype);  // true
console.log(pB === B.prototype);  // true
console.log(pA === A.prototype);  // true

// 5. Re-derive the same chain — fresh intermediate vars must
//    produce identical comparisons (proto identity is stable).
const pC2: any = Object.getPrototypeOf(c);
const pB2: any = Object.getPrototypeOf(pC2);
const pA2: any = Object.getPrototypeOf(pB2);
console.log(pA === pA2);  // true

// 6. Cross-chain identity: B.prototype reached from c's chain
//    must equal direct B.prototype.
console.log(pB === B.prototype);  // true (redundant check, sanity)
console.log(pA === Object.getPrototypeOf(B.prototype));  // true
