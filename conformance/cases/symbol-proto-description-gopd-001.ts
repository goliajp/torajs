// §20.4.3.2 — get Symbol.prototype.description: gOPD materializes
// the accessor descriptor; the getter runs thisSymbolValue.
const proto = (Symbol as any).prototype;
const d = Object.getOwnPropertyDescriptor(proto, "description")!;
console.log(typeof d.get);
console.log(d.set);
console.log(d.enumerable);
console.log(d.configurable);
console.log("value" in d);
console.log(d.get!.name);
console.log(d.get!.length);
const s = Symbol("hello");
console.log((d.get as any).call(s));
console.log((d.get as any).call(Symbol()));
try {
  (d.get as any).call(0);
} catch (e: any) {
  console.log(e instanceof TypeError);
}
console.log(Object.prototype.hasOwnProperty.call(proto, "description"));
