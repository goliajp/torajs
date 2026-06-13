// RFC 20260613 C1 — Object.defineProperty with a runtime (non-literal)
// descriptor object. Exercises __torajs_dynobj_define_from_desc, which
// reads value/writable/enumerable/configurable off the desc dynobj at
// runtime (the literal-descriptor path extracts them at compile time).
const o: any = {};
const desc: any = { value: 42, writable: false, enumerable: true, configurable: false };
Object.defineProperty(o, "x", desc);
console.log(o.x);
const g: any = Object.getOwnPropertyDescriptor(o, "x");
console.log(g.value, g.writable, g.enumerable, g.configurable);
let threw = false;
try {
  o.x = 99;
} catch (e) {
  threw = true;
}
console.log(threw, o.x);
const desc2: any = { value: "hi", writable: true, enumerable: true, configurable: true };
Object.defineProperty(o, "y", desc2);
console.log(o.y);
