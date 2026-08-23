// RFC 20260823-typedarray-substrate — own properties on a view.
// A TypedArray cell is an ordinary object off its index face
// (§23.2): plain assigns and defines land in a lazy expando bag,
// reads probe it own-first, and has / delete / keys / gOPD agree.
const ta: any = new Int8Array(4);

// plain assign + read-back
ta.foo = 41;
console.log("assign", ta.foo);

// define a data property; own "length" shadows the prototype accessor
Object.defineProperty(ta, "tagged", { value: 7, writable: true, enumerable: true, configurable: true });
console.log("define", ta.tagged);
Object.defineProperty(ta, "length", { value: 2 });
console.log("own-length", ta.length);

// accessor entry — the species shape: a getter installed ON THE INSTANCE
let hits = 0;
Object.defineProperty(ta, "probe", { get: () => { hits++; return 99; }, configurable: true });
console.log("getter", ta.probe, hits);

// throwing getter — the exact speciesctor-get-ctor-abrupt shape
Object.defineProperty(ta, "boom", { get: () => { throw new TypeError("from instance getter"); }, configurable: true });
try {
  ta.boom;
  console.log("no-throw");
} catch (e: any) {
  console.log("threw", e.message);
}

// has: expando key, in-bounds index, OOB index, prototype accessor absent as own
console.log("has", Object.hasOwn(ta, "foo"), Object.hasOwn(ta, "1"), Object.hasOwn(ta, "9"), Object.hasOwn(ta, "byteLength"));

// keys: indices first, then enumerable expando string keys
console.log("keys", Object.keys(ta).join(","));

// gOPD over the bag
const d: any = Object.getOwnPropertyDescriptor(ta, "tagged");
console.log("gopd", d.value, d.writable, d.enumerable, d.configurable);
console.log("gopd-proto", Object.getOwnPropertyDescriptor(ta, "byteLength"));

// delete: expando key succeeds, in-bounds index refuses (strict throw)
delete ta.foo;
console.log("deleted", ta.foo, Object.hasOwn(ta, "foo"));
try {
  delete ta[1];
  console.log("index-delete-ok");
} catch (e: any) {
  console.log("index-delete-threw");
}

// elements are untouched by any of this
console.log("elems", ta[0], ta[1], ta.byteLength);
