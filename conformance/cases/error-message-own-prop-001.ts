// RFC 20260718-error-message-own-prop 刀 1 — error-instance `message`
// own-property reflection semantics (§20.5.6.1.1): descriptor
// attributes {w:true, e:false, c:true}, enumeration exclusion,
// delete detach, any-receiver write.
const e1 = new TypeError("foo 42");
const d: any = Object.getOwnPropertyDescriptor(e1, "message");
console.log(d.value, d.writable, d.enumerable, d.configurable);
console.log(e1.propertyIsEnumerable("message"));
console.log(Object.keys(e1).includes("message"));
let seen = false;
for (const k in e1) {
  if (k === "message") seen = true;
}
console.log(seen);
console.log(Object.getOwnPropertyNames(e1).includes("message"));

// [[Writable]] via any receiver + delete detach + absent reflection
const ea: any = new RangeError("m1");
ea["message"] = "unlikelyValue";
console.log(ea.message);
console.log(delete ea.message);
console.log(ea.hasOwnProperty("message"));
console.log(Object.getOwnPropertyNames(ea).includes("message"));
console.log(Object.getOwnPropertyDescriptor(ea, "message") === undefined);

// subclass face — same message semantics through a user subclass
class Err extends TypeError {}
const e2 = new Err("sub msg");
const d2: any = Object.getOwnPropertyDescriptor(e2, "message");
console.log(d2.value, d2.writable, d2.enumerable, d2.configurable);
console.log(e2.propertyIsEnumerable("message"));
console.log(Object.keys(e2).includes("message"));
