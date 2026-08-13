// ES §25.5.2.1 step 4.b — an ARRAY replacer builds a PropertyList,
// and §25.5.2.4 step 5 uses it instead of the holder's own enumerable
// names. Ignoring it answered the unfiltered object, which is valid
// JSON and so invisible to every gate.
console.log(JSON.stringify({ a: 1, b: 2 }, ["a"]));

// The LIST's order wins, not the object's.
console.log(JSON.stringify({ a: 1, b: 2, c: 3 }, ["c", "a"]));

// Step 4.b.iv.f — a repeated name is appended once; a name the object
// does not have reads `undefined` and contributes nothing.
console.log(JSON.stringify({ a: 1, b: 2 }, ["a", "a", "zz"]));

// The list applies at every object depth.
console.log(JSON.stringify({ a: { b: 1, c: 2 }, d: 3 }, ["a", "b"]));

// SerializeJSONArray never consults it — elements are not filtered,
// but the objects inside them still are.
console.log(JSON.stringify([{ a: 1, b: 2 }], ["a"]));

// Step 4.b.iv.d — a Number element names its ToString.
console.log(JSON.stringify({ "1": "x", a: 1 }, [1]));

// Anything that is neither a String nor a Number names no property.
console.log(JSON.stringify({ a: 1, b: 2 }, [true, null, "b"]));

// space still applies alongside a list.
console.log(JSON.stringify({ a: 1, b: 2 }, ["a"], 2));

// The struct lane reads its declared fields through the same Get.
class Q {
  m: number = 4;
  n: number = 5;
}
console.log(JSON.stringify(new Q(), ["n"]));

// An `any`-typed binding holding the array is tested at run time.
const dyn: any = ["a"];
console.log(JSON.stringify({ a: 1, b: 2 }, dyn));
