// An object-literal / class accessor rides the layout under a
// synthetic slot name (`__getter_v` / `__setter_v`). That spelling is
// a compiler-internal name and must not surface as a property.
// `accessor_name_kind` is the authority and the read face has always
// asked it; the membership and descriptor faces did not.

const om = { a: 1, get b() { return 2; }, set c(v: any) {} };
const w: any = om;

// The property is there under its real name, on every face.
console.log(Object.hasOwn(w, "b"), Object.hasOwn(w, "c"), "b" in w, Reflect.has(w, "c"));
console.log(om.hasOwnProperty("b"), om.propertyIsEnumerable("b"));
console.log(JSON.stringify(Object.keys(w)));

// The spelling is not.
console.log(Object.hasOwn(w, "__getter_b"), Object.hasOwn(w, "__setter_c"));
console.log("__getter_b" in w, "__setter_c" in w);
console.log(Reflect.has(w, "__getter_b"), Reflect.has(w, "__setter_c"));
console.log(om.hasOwnProperty("__getter_b"), om.propertyIsEnumerable("__getter_b"));
console.log(Object.getOwnPropertyDescriptor(w, "__getter_b") === undefined);
console.log(Reflect.getOwnPropertyDescriptor(w, "__setter_c") === undefined);
console.log(JSON.stringify(Object.keys(Object.getOwnPropertyDescriptors(w))));
console.log(w.__getter_b, w.__setter_c);
console.log(JSON.stringify(Object.getOwnPropertyNames(w)), JSON.stringify(Reflect.ownKeys(w)));

// A class accessor is the prototype's property, so the leak had a
// second home one chain link over.
class C {
  get d() {
    return 1;
  }
  set e(v: any) {}
  m() {
    return 2;
  }
}
const c: any = new C();
console.log("d" in c, "e" in c, "m" in c);
console.log("__getter_d" in c, "__setter_e" in c, Reflect.has(c, "__getter_d"));
console.log(Object.hasOwn(c, "__getter_d"), c.hasOwnProperty("__getter_d"), c.__getter_d);
console.log(Object.getOwnPropertyDescriptor(C.prototype, "__getter_d") === undefined);
