// `Object.getOwnPropertyDescriptors` had no lowering at all — it did
// not answer wrong, it refused to compile. Its definition is the
// composition of two things that already existed: OwnPropertyKeys,
// then [[GetOwnProperty]] per key.
const o: any = { a: 1, b: "x" };
console.log("plain:", JSON.stringify(Object.getOwnPropertyDescriptors(o)));
console.log("empty:", JSON.stringify(Object.getOwnPropertyDescriptors({})));

// An array carries its indices AND its `length`, which is the one
// non-enumerable own property here — this is the OWN surface, not the
// enumerable one.
const arr: any = [1, 2];
console.log("array:", JSON.stringify(Object.getOwnPropertyDescriptors(arr)));

// A class instance answers its declared fields, its expandos, and a
// defineProperty'd entry with the attributes it was given.
class Box {
  v: number = 1;
}
const c: any = new Box();
c.zz = 9;
Object.defineProperty(c, "hid", { value: 5, enumerable: false });
console.log("instance:", JSON.stringify(Object.getOwnPropertyDescriptors(c)));

// Symbol keys are own properties too, and OwnPropertyKeys returns
// them — JSON.stringify cannot show them, so read one back directly.
const s = Symbol("k");
const withSym: any = { a: 1 };
withSym[s] = 2;
const ds: any = Object.getOwnPropertyDescriptors(withSym);
console.log("symbol value:", ds[s].value);
console.log("symbol kept string key:", JSON.stringify(ds["a"]));

// §20.1.2.9 step 1 is ToObject, which throws for the two nullish values.
for (const bad of [null, undefined]) {
  try {
    Object.getOwnPropertyDescriptors(bad);
    console.log("no throw for", String(bad));
  } catch (e: any) {
    console.log("threw for", String(bad) + ":", e instanceof TypeError);
  }
}

// A string receiver reaches the descriptor kernel, which had no arm
// for one: the singular form only ever answered through compile-time
// fast paths that need a literal key AND a statically-typed receiver.
const sv: any = "ab";
console.log("string descs:", JSON.stringify(Object.getOwnPropertyDescriptors(sv)));
// The same gap, in the singular: a runtime key, and an `any` receiver.
const k: any = "0";
console.log("string runtime key:", JSON.stringify(Object.getOwnPropertyDescriptor(sv, k)));
console.log("string any recv:", JSON.stringify(Object.getOwnPropertyDescriptor(sv, "1")));
console.log("string length:", JSON.stringify(Object.getOwnPropertyDescriptor(sv, "length")));
console.log("string absent:", Object.getOwnPropertyDescriptor(sv, "9"));
