// rotation 559 — a Symbol key reaching the struct-receiver name
// lookups (`in` / hasOwnProperty / gOPD / member read / define) is not
// a Str cell: the WTF-8 key boundary must not read Str payload
// offsets off it (exit 139 on 19 test262 key-is-symbol cases after
// ac78e6186). A Symbol names no baked field, so every probe answers
// the expando lane's verdict.
const o = { a: 1, "": 2 };
const s = Symbol("k");
console.log(s in o, o.hasOwnProperty(s), Object.getOwnPropertyDescriptor(o, s), (o as any)[s]);
(o as any)[s] = 3;
console.log((o as any)[s], s in o, o.hasOwnProperty(s), Object.keys(o).length, o[""]);
Object.defineProperty(o, s, { value: 4, enumerable: false });
console.log((o as any)[s], Object.getOwnPropertyDescriptor(o, s)!.enumerable);
const t = new Uint8Array(2);
console.log(s in t, t.hasOwnProperty(Symbol.iterator), Symbol.iterator in t);
console.log(typeof Object.getOwnPropertyDescriptor(Array.prototype, Symbol.iterator)!.value);
console.log(Object.getOwnPropertyDescriptor(Array.prototype, Symbol.unscopables)!.enumerable);
