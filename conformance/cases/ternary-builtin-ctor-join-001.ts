// 633-01 — two DIFFERENT builtin constructors read as VALUES join to
// Any in a ternary, the S129-1 posture the Struct / mixed-scalar arms
// already take. `Type::Object(name)` is the constructor, not an
// instance of it: both branches box the same interned ctor cell, so
// there is no layout to reconcile. test262's typed-array harness is
// built out of this shape and used to reject with "ternary branches
// differ — `then` is Object(\"Float64Array\"), `else` is
// Object(\"Float32Array\")".
function opaque(n: number): any {
  return n;
}
const yes: any = opaque(1);
const no: any = opaque(0);

const Wide: any = yes ? Float64Array : Float32Array;
const Narrow: any = no ? Float64Array : Float32Array;
const w: any = new Wide(2);
const n: any = new Narrow(2);
w[0] = 0.1;
n[0] = 0.1;
console.log("ctor", Wide.name, Narrow.name);
console.log("bpe", Wide.BYTES_PER_ELEMENT, Narrow.BYTES_PER_ELEMENT);
console.log("precision", w[0] === 0.1, n[0] === 0.1);

// A static reached off the joined value.
const Buf: any = yes ? ArrayBuffer : DataView;
console.log("static", Buf.name, Buf.isView(w));

// The join carries instanceof and construction alike.
const Coll: any = no ? Map : Set;
const c: any = new Coll();
c.add(7);
console.log("coll", Coll.name, c.size, c instanceof Set, c instanceof Map);

// Length / name off the constructor value itself.
const Prim: any = yes ? Number : String;
console.log("prim", Prim.name, Prim.length, Prim("12") + 1);
