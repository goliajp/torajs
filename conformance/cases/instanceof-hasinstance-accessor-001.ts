// §13.10.2 step 2 is GetMethod(target, @@hasInstance), which is a Get —
// so an accessor-shaped handler has to run its getter. The symbol probe
// answers an ACCESSOR sentinel and this arm tested it for "heap value",
// so the handler was silently dropped and the ordinary prototype walk
// answered instead. The module's own doc claimed this lookup and the
// expression form `C[Symbol.hasInstance]` "cannot disagree" — under an
// accessor they did: the expression ran the getter, instanceof did not.

// accessor-shaped handler
const A: any = function () {};
Object.defineProperty(A, Symbol.hasInstance, {
  get() { return (_x: any) => true; },
});
console.log(({} as any) instanceof A); // true
console.log((42 as any) instanceof A); // true

// the expression spelling agrees with the operator
const seen: any = A[Symbol.hasInstance];
console.log(typeof seen); // function

// a handler answering false
const B: any = function () {};
Object.defineProperty(B, Symbol.hasInstance, {
  get() { return (_x: any) => false; },
});
console.log(({} as any) instanceof B); // false

// data-property handler still works
const C: any = function () {};
Object.defineProperty(C, Symbol.hasInstance, { value: (_x: any) => true });
console.log(({} as any) instanceof C); // true

// no handler at all falls back to the prototype walk
class D {}
console.log(new D() instanceof D, ({} as any) instanceof D); // true false
