// Rotation 207 — ES §25.5.2 SerializeJSONProperty on the struct
// lane. Rotation 205 shipped the toJSON hook for the any lane; a
// statically typed object literal carrying one still reached the
// field unfold and rejected on its Closure slot ("JSON.stringify on
// type Closure"). Two spec steps land here: step 2 consults a
// callable `toJSON` and serializes its result, and step 11 makes any
// other callable property serialize to undefined, which step 8.b
// then omits.

// Step 11 — a plain method field is omitted, not rejected.
const g = { v: 7, f() { return 1; } };
console.log("A", JSON.stringify(g));

// Step 2 — the hook replaces the whole value. All three spellings.
const a = { v: 1, toJSON() { return "custom-a"; } };
console.log("B", JSON.stringify(a));
const b = { v: 2, toJSON: function () { return "custom-b"; } };
console.log("C", JSON.stringify(b));
const c = { v: 3, toJSON: () => "custom-c" };
console.log("D", JSON.stringify(c));

// Nested value and array element both consult it.
const d = { inner: { v: 4, toJSON() { return "custom-d"; } } };
console.log("E", JSON.stringify(d));
const f = [{ toJSON() { return "in-arr"; } }];
console.log("F", JSON.stringify(f));

// Step 2.b passes the key the value sits under — the property name
// for an object member, the index for an array element, "" at the
// top level.
const e = { k: { toJSON(key: string) { return "key=" + key; } } };
console.log("G", JSON.stringify(e));
const arr = [{ toJSON(key: string) { return "idx=" + key; } }];
console.log("H", JSON.stringify(arr));
const top = { toJSON(key: string) { return "top=[" + key + "]"; } };
console.log("I", JSON.stringify(top));

// A hook may answer a composite, which serializes in turn.
const h = { toJSON() { return { z: 8 }; } };
console.log("J", JSON.stringify(h));

// `this` inside the hook still binds the receiver.
const k = { n: 5, toJSON() { return this.n * 2; } };
console.log("K", JSON.stringify(k));

// Objects without a hook are unchanged.
const plain = { v: 7, w: "x" };
console.log("L", JSON.stringify(plain));
console.log("M", JSON.stringify([1, "two", true]));
console.log("N", JSON.stringify({ nested: { deep: 1 } }));
