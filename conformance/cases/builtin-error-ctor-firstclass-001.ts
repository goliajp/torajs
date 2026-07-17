// RFC 20260718-builtin-error-ctor-first-class 刀 1 — §15.7.14 class
// heritage on the ctor chain (`getPrototypeOf(Sub) === Super`;
// §20.5.6.2 NativeError.[[Prototype]] = Error is the same rule) and
// the §20.5.6.3/6.4 own `name` / `message` data properties on the
// injected error prototypes. A user subclass keeps the spec shape:
// its prototype carries neither (inherited), and its ctor chains to
// its parent ctor.
console.log(Object.getPrototypeOf(RangeError) === Error);
console.log(Object.getPrototypeOf(TypeError) === Error);
console.log(Object.getPrototypeOf(Error) === Function.prototype);
console.log((TypeError.prototype as any).name);
console.log((Error.prototype as any).name);
const d: any = Object.getOwnPropertyDescriptor(Error.prototype, "name");
console.log(d === undefined ? "no-desc" : d.value + "|" + d.writable + "|" + d.enumerable + "|" + d.configurable);
const d2: any = Object.getOwnPropertyDescriptor(TypeError.prototype, "message");
console.log(d2 === undefined ? "no-desc" : d2.value + "|" + d2.writable + "|" + d2.enumerable + "|" + d2.configurable);
const e = new TypeError("x");
console.log(e.name, e.message);
console.log(e instanceof TypeError, e instanceof Error);
class A {}
class B extends A {}
console.log(Object.getPrototypeOf(B) === A);
console.log(Object.getPrototypeOf(A) === Function.prototype);
class MyE extends Error {}
console.log(Object.getPrototypeOf(MyE) === Error);
console.log(MyE.prototype.hasOwnProperty("name"));
console.log((new MyE("z") as any).name);
