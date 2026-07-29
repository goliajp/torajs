// A Symbol key must never have the Str payload offsets read off its
// cell. The two layouts collide exactly where it hurts: a Str keeps
// `len: u64` at offset 8, a Symbol keeps its *description pointer*
// there — so reading blind hands a heap address out as a byte count.
//
// `defineProperty` with a symbol key is the shape that turned that
// into a fault: the builtin-prototype monkey-patch note walked the
// "name" behind the key, and the resulting multi-gigabyte span ran
// off the end of the mapping. Whether it faulted depended on where
// the heap happened to sit, so it stayed invisible until an unrelated
// change moved the layout. The same blind read sat in the array-index
// key scan and the built-in slot-name compare.
//
// Reading BACK a symbol-keyed defineProperty is a separate gap (tr
// answers undefined where bun answers the value) and is deliberately
// not asserted here — this case is about the define call not walking
// the key cell as a string, and about symbol keys staying out of
// every string-key face.

const s1: any = Symbol("alpha");
const s2: any = Symbol();

// The define path — a plain object, then the builtin prototype the
// monkey-patch note actually watches.
const o: any = {};
Object.defineProperty(o, s1, { value: 11, writable: true, configurable: true });
Object.defineProperty(String.prototype, s1, { value: 22, configurable: true });

// A symbol with no description — its offset-8 slot holds a null
// pointer rather than a heap address, the other half of the hazard.
Object.defineProperty(o, s2, { value: 33, writable: true, configurable: true });

// The plain-set twin of that path, on the same cells.
o[s2] = 33;
console.log(o[s2]);
o[s1] = 44;
console.log(o[s1]);

// Enumeration must not mistake a symbol for an array index, and must
// not walk its cell as digits. String keys keep their integer order.
const e: any = {};
e[s1] = "sym";
e["2"] = "two";
e["0"] = "zero";
e["b"] = "bee";
console.log(Object.keys(e).join(","));
console.log(Object.keys(e).length);

let seen = "";
for (const k in e) {
  seen = seen + k + ";";
}
console.log(seen);
console.log(JSON.stringify(e));

// Integrity walks read the same key cells looking for the built-in
// slot names; a symbol names none of them.
Object.freeze(e);
console.log(Object.isFrozen(e), e[s1]);

// An Array receiver routes defines to its own kernel before any key
// inspection, so it walks the key cell on its own — a symbol names
// neither `length` nor an index and belongs on the ordinary-key arm.
const arr: any = [1, 2, 3];
Object.defineProperty(arr, s1, { value: 55, writable: true, configurable: true });
arr[s1] = 66;
console.log(arr[s1], arr.length, arr.join(","));
console.log(Object.keys(arr).join(","));

// §7.1.19 step 2 applies to a symbol sitting in an `any` just as much
// as to a statically-typed one. Stringifying it stored the property
// under "Symbol(x)" — wrong key, and a name in Object.keys that no
// symbol ever has.
const typedSym = Symbol("typed");
const d: any = {};
Object.defineProperty(d, typedSym, { value: 1, writable: true, enumerable: true, configurable: true });
Object.defineProperty(d, s1, { value: 2, writable: true, enumerable: true, configurable: true });
console.log(d[typedSym], d[s1]);
console.log(JSON.stringify(Object.keys(d)));

// getOwnPropertyDescriptor carried its own copy of the same key
// resolution, and so the same hole.
const gt: any = Object.getOwnPropertyDescriptor(d, typedSym);
const gb: any = Object.getOwnPropertyDescriptor(d, s1);
console.log(gt === undefined ? "undefined" : gt.value, gb === undefined ? "undefined" : gb.value);

// `in` resolved its key by static type too, and rejected the whole
// program for an `any` key rather than answering.
console.log(s1 in d, typedSym in d, ("nope" as any) in d);

// delete resolved its key the same way, and refused the program for
// an `any` key; stringifying one would have deleted "Symbol(x)" and
// left the real entry in place.
const del: any = {};
del[typedSym] = 1;
del[s1] = 2;
console.log(Object.getOwnPropertySymbols(del).length);
console.log(delete del[s1], delete del[typedSym]);
console.log(Object.getOwnPropertySymbols(del).length, del[s1], del[typedSym]);

// Object.hasOwn pinned its key to `string`, so a symbol key was
// refused outright — even a statically-typed one, which the lowering
// had always handled.
const h: any = {};
h[typedSym] = 1;
h[s1] = 2;
console.log(Object.hasOwn(h, typedSym), Object.hasOwn(h, s1), Object.hasOwn(h, "nope"));
