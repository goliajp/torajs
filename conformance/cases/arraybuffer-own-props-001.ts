// RFC 20260823-typedarray-substrate — own properties on an
// ArrayBuffer (§25.1): the same lazy expando bag as the view, no
// index face at all.
const ab: any = new ArrayBuffer(8);

ab.note = "mine";
console.log("assign", ab.note);

Object.defineProperty(ab, "marked", { value: true, enumerable: true, configurable: true });
console.log("define", ab.marked);

let reads = 0;
Object.defineProperty(ab, "ctor", { get: () => { reads++; return "C"; }, configurable: true });
console.log("getter", ab.ctor, reads);

console.log("has", Object.hasOwn(ab, "note"), Object.hasOwn(ab, "byteLength"));
console.log("keys", Object.keys(ab).join(","));

const d: any = Object.getOwnPropertyDescriptor(ab, "marked");
console.log("gopd", d.value, d.enumerable);

delete ab.note;
console.log("deleted", ab.note, Object.hasOwn(ab, "note"));

// freeze refuses new expando writes
Object.freeze(ab);
try {
  ab.later = 1;
  console.log("frozen-write", ab.later);
} catch (e: any) {
  console.log("frozen-threw");
}
console.log("end", ab.byteLength);
