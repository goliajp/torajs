// §28.1.6 `Reflect.get` folds at compile time when the target is a
// typed struct and the key is a literal — but a layout scan can only
// prove PRESENCE. Absence is not a constant, and the fold answered
// `undefined` for both things the scan cannot see: an accessor, which
// lives in the layout under a synthetic slot name, and a class member,
// which is not in the instance layout at all.

const o = { a: 1, get b() { return 2; }, set c(v: any) {} };
console.log(Reflect.get(o as any, "a"), Reflect.get(o as any, "b"));
// A set-only property's [[Get]] is undefined, and so is an absent key.
console.log(Reflect.get(o as any, "c"), Reflect.get(o as any, "absent"));
// The synthetic spelling is not a property and must not fold to the
// closure sitting in the slot.
console.log(Reflect.get(o as any, "__getter_b"), Reflect.get(o as any, "__setter_c"));

// The same values through the spellings that always answered.
console.log(o.b, (o as any)["b"], JSON.stringify(Object.values(o as any)));

// A class accessor is the prototype's and sees its receiver.
class C {
  a = 3;
  get d() {
    return this.a + 10;
  }
  m() {
    return 7;
  }
}
const c = new C();
console.log(Reflect.get(c as any, "a"), Reflect.get(c as any, "d"));
console.log(typeof Reflect.get(c as any, "m"), Reflect.get(c as any, "toString") !== undefined);

// The data-field fold itself, unchanged — including a refcounted field.
const t = { n: 1, s: "str", arr: [1, 2] };
console.log(Reflect.get(t as any, "n"), Reflect.get(t as any, "s"));
console.log(JSON.stringify(Reflect.get(t as any, "arr")));

// A throwing getter propagates.
const th = { get x() { throw new TypeError("boom"); } };
try {
  Reflect.get(th as any, "x");
  console.log("no throw");
} catch (e) {
  console.log("threw", e instanceof TypeError);
}
