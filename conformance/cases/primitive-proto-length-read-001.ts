// §10.1.8.1 — `.length` on a receiver with no inherent length is an ordinary
// [[Get]]: a `length` installed on Number.prototype / Boolean.prototype is read
// through a primitive, through its wrapper object, and through every spelling
// (`o.length`, `o["length"]`, `o[k]`, Reflect.get) alike. Own expando on the
// wrapper still wins, and a String wrapper keeps its inherent length.
(Number.prototype as any).length = 2;
(Boolean.prototype as any).length = 1;
const n: any = 1;
const o: any = Object(n);
const k: any = "length";
console.log(o.length, o["length"], o[k], Reflect.get(o, "length"), n.length);
const b: any = Object(true);
console.log(b.length, b["length"], (true as any).length);
Object.defineProperty(o, "length", { value: 9, configurable: true });
console.log(o.length, Object(2).length);
const s: any = Object("abc");
console.log(s.length, "abc".length);
const m: any = new Map();
console.log(m.length);
(Map.prototype as any).length = 3;
console.log(m.length);
console.log("done");
