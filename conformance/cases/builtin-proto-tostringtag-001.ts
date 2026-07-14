// §20.1.3.6 — the "[object X]" badge of a builtin prototype. Five of
// them carry a well-known `Symbol.toStringTag` (Map / Set / Promise /
// Symbol / BigInt); the primitive-wrapper and exotic ones answer for
// what they ARE (a Number object, an Array, a callable). The genuinely
// ordinary ones — Object / RegExp / Date / Error — stay "[object
// Object]", which is bun's answer too.
const T = (x: any) => Object.prototype.toString.call(x);

console.log(T(Map.prototype), T(Set.prototype), T(Promise.prototype));
console.log(T(Symbol.prototype), T(BigInt.prototype));
console.log(T(Number.prototype), T(String.prototype), T(Boolean.prototype));
console.log(T(Array.prototype), T(Function.prototype));
// (`RegExp.prototype` would belong on this line, but `RegExp` is not
// a known identifier to tr's checker yet — a separate hole.)
console.log(T(Object.prototype), T(Date.prototype));

// The same badge through the inherited method, not the .call() form:
// `Map.prototype.toString()` finds Object.prototype.toString and reads
// the tag off its receiver.
console.log((Map.prototype as any).toString(), (Set.prototype as any).toString());
console.log((Promise.prototype as any).toString());
console.log((Object.prototype as any).toString());

// Instances keep answering for their own shape.
console.log(T(new Map()), T(new Set()), T([1]), T({ a: 1 }));
