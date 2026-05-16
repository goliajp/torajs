// P3.attribute-flag-tracking — defineProperty must honor
// writable/configurable/enumerable per spec §6.2.5 / §10.1.5 /
// §10.1.6, not silently no-op them. Pre-fix tora's P3.3
// defineProperty intercept stores only the .value field and ignores
// the flag attributes, so non-writable / non-configurable property
// definitions silently fail to enforce on subsequent set/redefine
// — a quality-HARD silent-wrong (looks like spec compliance but
// drops the constraints).
//
// Scope (first ship): writable=false rejects subsequent assignment,
// configurable=false rejects subsequent redefinition. Enumerable +
// Object.keys / for-in interaction is a follow-up (needs
// Object.keys enumerable-aware walk; deferred to subsequent ship).

// writable: false — subsequent assignment must throw TypeError
// (§10.1.5.2 OrdinaryDefineOwnProperty + §10.1.5 ordinary [[Set]]).
let a: any = {};
Object.defineProperty(a, "x", { value: 1, writable: false, configurable: true, enumerable: true });
console.log(a.x);  // 1
let threw1: boolean = false;
try {
  a.x = 2;
} catch (e: any) {
  threw1 = true;
}
console.log(threw1);   // true
console.log(a.x);      // 1  (unchanged after the failed assignment)

// writable=false + configurable=false — defineProperty trying to
// change the value throws (§10.1.6.3 step "If current.[[Writable]]
// is false, then if Desc.[[Value]] is present and SameValue with
// current.[[Value]] is false, return false → caller throws").
let b: any = {};
Object.defineProperty(b, "y", { value: 1, writable: false, configurable: false, enumerable: true });
console.log(b.y);  // 1
let threw2: boolean = false;
try {
  Object.defineProperty(b, "y", { value: 2 });
} catch (e: any) {
  threw2 = true;
}
console.log(threw2);   // true
console.log(b.y);      // 1  (unchanged after the failed redefine)

// configurable=false → cannot upgrade configurable back to true
// (spec: configurable transitions are one-way false; reverse throws).
let d: any = {};
Object.defineProperty(d, "k", { value: 1, writable: true, configurable: false, enumerable: true });
let threw3: boolean = false;
try {
  Object.defineProperty(d, "k", { value: 1, writable: true, configurable: true, enumerable: true });
} catch (e: any) {
  threw3 = true;
}
console.log(threw3);   // true

// Sanity: writable=true configurable=true (the default-loose case)
// — both assignment AND redefinition succeed.
let c: any = {};
Object.defineProperty(c, "z", { value: 1, writable: true, configurable: true, enumerable: true });
c.z = 100;
console.log(c.z);  // 100
Object.defineProperty(c, "z", { value: 200, writable: true, configurable: true, enumerable: true });
console.log(c.z);  // 200
