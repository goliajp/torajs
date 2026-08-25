// RFC 20260825-inject-narrow-define 刀 4a — the prologue's proto-chain
// wire (`__proto_<Sub>` → `__proto_<Super>`) moved from a generic
// `.__proto__ = …` member assign to the narrow
// `__torajs_proto_link_fresh` kernel. These reads walk the exact
// entries that kernel writes: the user-class chain, the injected
// native-error chain, and inheritance through the link.
class A {
  greet(): string {
    return "a";
  }
}
class B extends A {}
class C extends B {}

console.log(Object.getPrototypeOf(B.prototype) === A.prototype);
console.log(Object.getPrototypeOf(C.prototype) === B.prototype);
console.log(new C().greet());
console.log(new C() instanceof A);

// The injected native-error family rides the same wire.
console.log(Object.getPrototypeOf(TypeError.prototype) === Error.prototype);
console.log(Object.getPrototypeOf(RangeError.prototype) === Error.prototype);
const te = new TypeError("boom");
console.log(te instanceof Error);
console.log(te.message);

// A user subclass of an injected class chains through both wires.
class MyErr extends TypeError {}
const me = new MyErr("mine");
console.log(me instanceof TypeError);
console.log(me instanceof Error);
console.log(me.message);
