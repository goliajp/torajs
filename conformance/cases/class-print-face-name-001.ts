// 567-01 — `console.log(<class>)` prints the class's DEFINITION
// name, not its own `name` property. The two come apart the moment
// anything redefines `name`: JSC's inspect asks the class about the
// source it was written in, so `[class C]` survives a rename that
// `C.name` reports faithfully.

class Named {
  x = 1;
}
const N: any = Named;
console.log(N, JSON.stringify(N.name));
Object.defineProperty(N, "name", { value: "qq", configurable: true });
console.log(N, JSON.stringify(N.name));

// The `extends` half reads the same table: renaming the PARENT does
// not move the spelling the child prints.
class Sub extends Named {}
const S: any = Sub;
console.log(S);
Object.defineProperty(N, "name", { value: "ww", configurable: true });
console.log(S, JSON.stringify(S.name));

// A class expression's definition name is §8.4.5 NamedEvaluation's
// verdict at the binding — "A" here, the empty string with no
// binding at all, and its own spelling always wins (§15.5.5).
const A: any = class {};
console.log(A, JSON.stringify(A.name));
Object.defineProperty(A, "name", { value: "rr", configurable: true });
console.log(A, JSON.stringify(A.name));
const B: any = class Inner {};
console.log(B, JSON.stringify(B.name));

// A field-less class still has a row in the name table.
class Bare {}
const R: any = Bare;
Object.defineProperty(R, "name", { value: "zz", configurable: true });
console.log(R, JSON.stringify(R.name));

// Extending a builtin names the builtin, which has no row of its
// own and answers through the interned ctor cell instead.
class P extends Array {}
console.log(P);

// Nested and inside a container, the same form at every depth.
console.log([N, S, A]);
