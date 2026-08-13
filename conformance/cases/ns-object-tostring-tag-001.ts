// §21.3.1.9 / §25.5.3 / §28.1.14 + Web IDL — the namespace objects
// carry a real @@toStringTag, not a badge keyed on their identity.
const T: any = Symbol.toStringTag;
const ts: any = Object.prototype.toString;

function desc(label: string, o: any): void {
  const d: any = Object.getOwnPropertyDescriptor(o, T);
  if (d === undefined) {
    console.log(label + " MISSING");
  } else {
    console.log(label + " " + d.value + " w=" + d.writable + " e=" + d.enumerable + " c=" + d.configurable);
  }
}

desc("Math", Math);
desc("JSON", JSON);
desc("Reflect", Reflect);
desc("console", console);

// the badge is derived from that property
console.log(ts.call(Math));
console.log(ts.call(JSON));
console.log(ts.call(Reflect));

// non-enumerable, so it stays out of the string-key surfaces
console.log(Object.keys(Math).length > 0);
console.log(Object.getOwnPropertyNames(JSON).indexOf("Symbol(Symbol.toStringTag)"));
console.log(Object.getOwnPropertySymbols(Math).length);

// configurable -- and once deleted the badge has to go with it,
// which is the whole reason it cannot be an identity check
console.log(Reflect.deleteProperty(JSON, T));
console.log(Object.getOwnPropertyDescriptor(JSON, T) === undefined);
console.log(ts.call(JSON));

// the other namespaces are untouched by that delete
console.log(ts.call(Math));
console.log(ts.call(Reflect));
console.log(ts.call(console));
