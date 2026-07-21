// §7.1.18 ToObject(Symbol) — Object(sym) mints a SymbolWrapper:
// typeof "object", disjoint identity, thisSymbolValue view-through.
const s = Symbol("hi");
const o = Object(s);
console.log(typeof o);
console.log(o === (s as any));
console.log(o.toString());
console.log(o.valueOf() === s);
console.log(o.description);
console.log((Symbol.prototype.toString as any).call(o));
const e = Object(Symbol());
console.log(e.description);
console.log(e.toString());
console.log(Object(Symbol.iterator).toString());
console.log(Object(Symbol.for("dummies")).toString());
try {
  (Symbol.prototype.toString as any).call(new String("still-not-ok"));
} catch (err: any) {
  console.log(err instanceof TypeError);
}
