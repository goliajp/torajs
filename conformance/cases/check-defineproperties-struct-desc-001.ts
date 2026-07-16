// §6.2.6.5 ToPropertyDescriptor over a static-layout (Tag::Obj)
// descriptor cell — `props.p = { value: 42, ... }` without `as any`
// types structurally, so the runtime desc lane must read class-layout
// struct fields, not just dynobj entries.
const o1: any = {};
const props1: any = {};
props1.p = { value: 42, enumerable: true };
Object.defineProperties(o1, props1);
console.log(o1.p);
console.log(Object.keys(o1).length);
const d1: any = Object.getOwnPropertyDescriptor(o1, "p");
console.log(d1.value, d1.writable, d1.enumerable, d1.configurable);

const o2: any = {};
const props2: any = {};
props2.q = { value: "s", writable: true, configurable: true };
Object.defineProperties(o2, props2);
const d2: any = Object.getOwnPropertyDescriptor(o2, "q");
console.log(d2.value, d2.writable, d2.enumerable, d2.configurable);

const o3: any = Object.create({}, props1);
console.log(o3.p);

const o4: any = {};
const props4: any = {};
props4.g = { get: function () { return 7; }, enumerable: true };
Object.defineProperties(o4, props4);
console.log(o4.g);

const o5: any = {};
const props5: any = {};
props5.b = { get: function () { return 1; }, value: 2 };
try {
  Object.defineProperties(o5, props5);
} catch (e: any) {
  console.log("mix:", e.message);
}
