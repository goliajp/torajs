// §20.1.2.3.1 ObjectDefineProperties walks the own enumerable keys of
// ANY object as its Properties argument, and §6.2.6.5 accepts ANY object
// as a descriptor — a Date, a RegExp, an Error, a Promise, an array, a
// wrapper, a function, all carrying their fields as expandos (test262
// Object/create 15.2.3.5-4-11 / 4-11x).
function desc(o: any, k: string) { const d = Object.getOwnPropertyDescriptor(o, k); return d ? [d.value, d.writable, d.enumerable, d.configurable].join("/") : "none"; }
// props container of assorted kinds
const props1: any = new Date(0);
Object.defineProperty(props1, "prop", { get: function () { return { value: 1, enumerable: true }; }, enumerable: true });
const o1: any = Object.create({}, props1);
console.log("date-container", desc(o1, "prop"));
const props2: any = new RegExp("x"); props2.p = { value: 2, writable: true };
const props3: any = new Error("e"); props3.p = { value: 3, configurable: true };
const props4: any = Promise.resolve(0); props4.p = { value: 4 };
console.log("regexp/error/promise-container", desc(Object.create({}, props2), "p"), desc(Object.create({}, props3), "p"), desc(Object.create({}, props4), "p"));
// descriptors of assorted kinds
const d1: any = new Date(0); d1.value = "dv"; d1.enumerable = true;
const d2: any = new RegExp("y"); d2.value = "rv"; d2.configurable = true;
const d3: any = new Error("m"); d3.value = "ev"; d3.writable = true;
const d4: any = [1, 2]; d4.value = "av";
const d5: any = new Number(-9); d5.configurable = true;
const d6: any = function () {}; d6.value = "fv"; d6.enumerable = true;
const o2: any = Object.create({}, { a: d1, b: d2, c: d3, d: d4, e: d5, f: d6 });
console.log("kinds", desc(o2, "a"), desc(o2, "b"), desc(o2, "c"), desc(o2, "d"), desc(o2, "e"), desc(o2, "f"));
console.log("keys", Object.keys(o2).join(","), delete o2.b, o2.hasOwnProperty("b"));
// defineProperties with the same shapes
const o3: any = {};
Object.defineProperties(o3, { x: d2, y: props2.p });
console.log("defineProperties", desc(o3, "x"), desc(o3, "y"));
// primitives are still refused
for (const bad of ["str", 1, true, null, undefined]) {
  try { Object.create({}, { p: bad as any }); console.log("accepted", typeof bad); } catch (e) { console.log("refused", typeof bad, (e as any) instanceof TypeError); }
}
// an inherited accessor field with only a setter reads as undefined -> false
const proto: any = {};
Object.defineProperty(proto, "configurable", { get: function () { return true; } });
const Con: any = function () {};
Con.prototype = proto;
const dobj: any = new Con();
Object.defineProperty(dobj, "configurable", { set: function () {} });
const o4: any = Object.create({}, { prop: dobj });
console.log("setter-only", desc(o4, "prop"));
