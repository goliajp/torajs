// chunk D-1 (RFC 20260711 propertyHelper) — hasOwnProperty /
// propertyIsEnumerable universal any-method arms + Object.hasOwn on
// any receivers (ES §20.1.4.3 / §20.1.4.5 / §20.1.2.4).
const o: any = { a: 1 };
console.log(o.hasOwnProperty("a"), o.hasOwnProperty("z"));
console.log(o.propertyIsEnumerable("a"), o.propertyIsEnumerable("z"));
Object.defineProperty(o, "h", { value: 9, enumerable: false });
console.log(o.hasOwnProperty("h"), o.propertyIsEnumerable("h"));
const a: any = [1, 2];
console.log(a.hasOwnProperty("0"), a.hasOwnProperty("2"), a.hasOwnProperty("length"));
console.log(a.propertyIsEnumerable("0"), a.propertyIsEnumerable("length"));
a.x = 5;
console.log(a.hasOwnProperty("x"), a.propertyIsEnumerable("x"));
const s: any = "ab";
console.log(s.hasOwnProperty("1"), s.hasOwnProperty("5"), s.hasOwnProperty("length"));
console.log(s.propertyIsEnumerable("0"), s.propertyIsEnumerable("length"));
const f: any = () => 1;
f.z = 3;
console.log(f.hasOwnProperty("z"), f.hasOwnProperty("q"));
const n: any = 42;
console.log(n.hasOwnProperty("x"), n.propertyIsEnumerable("x"));
console.log(Object.hasOwn({ a: 1 } as any, "a"));
const p: any = { x: 1 };
const k = "x";
console.log(Object.hasOwn(p, k));
console.log(Object.hasOwn(p, "absent"));
console.log("done");
