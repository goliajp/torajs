// §15.7.14 — every ComputedPropertyName evaluates once, in element
// order, at class-definition time; the static field INITIALIZERS then
// run interleaved with the static blocks in their own declaration
// order. This rotation moved where the computed static stores are
// emitted, so the order is worth pinning.
const log: string[] = [];
function key(n: string) { log.push("key:" + n); return n; }
function val(n: string) { log.push("val:" + n); return n; }
class C {
  static a = val("a");
  static [key("b")] = val("b");
  static { log.push("block"); }
  static c = val("c");
  [key("d")]() { return 1; }
  static [key("e")] = val("e");
}
console.log(log.join(" "));
console.log(Object.getOwnPropertyNames(C).filter(n => n !== "length" && n !== "name" && n !== "prototype").join(","));
// every element kind with a computed name, in one class
const log2: string[] = [];
function k2(n: string) { log2.push("k" + n); return n; }
class D { [k2("1")]() {} static [k2("2")]() {} get [k2("3")]() { return 1; } set [k2("4")](_: number) {} static [k2("5")] = 9; [k2("6")] = 8; }
console.log(log2.join(","));
const d: any = new D();
console.log(Object.keys(d).join("/"), (D as any)["5"]);
// a symbol key on both halves
const s1 = Symbol("s1");
class E { [s1]() { return "sym"; } static [s1] = 5; }
const e: any = new E();
console.log(typeof e[s1], e[s1](), (E as any)[s1]);
console.log(Object.getOwnPropertySymbols(E.prototype).length, Object.getOwnPropertySymbols(E).length);
