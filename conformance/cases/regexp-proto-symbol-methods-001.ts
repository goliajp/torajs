// RegExp.prototype carries its five symbol-keyed protocol methods as own
// properties (§22.2.6), the same functions an instance reads; called
// with a non-object `this` they throw a TypeError before anything else.
const P: any = RegExp.prototype;
const r: any = /a/;
console.log(typeof P[Symbol.search], typeof P[Symbol.split], typeof P[Symbol.match], typeof P[Symbol.matchAll], typeof P[Symbol.replace]);
console.log(P[Symbol.search] === r[Symbol.search], P[Symbol.split].length, P[Symbol.replace].length);
console.log(Object.getOwnPropertySymbols(RegExp.prototype).map((s) => String(s)).join(","));
const d = Object.getOwnPropertyDescriptor(RegExp.prototype, Symbol.search)!;
console.log(d.writable, d.enumerable, d.configurable);
console.log(P[Symbol.search].call(/b/, "abc"), P[Symbol.split].call(/,/, "a,b").join("|"));
for (const v of [undefined, null, true, "s", Symbol.search, 86] as any[]) {
  try {
    P[Symbol.search].call(v, "x");
    console.log("no throw");
  } catch (e: any) {
    console.log(e.constructor.name);
  }
}
