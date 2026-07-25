// `o[sym] = v` stores into the symbol-keyed slot — the write half of
// the §6.1.7 property-key domain, twin of the read lane.
//
// The runtime write does not thread symbol keys through the string
// cascade either: every arm there is guarded by a property NAME —
// `key_is(key, "lastIndex")`, the `name` / `length` readonly refusals,
// a builtin ctor's non-writable `prototype`, the Symbol constructor's
// well-known statics, `length` resize, canonical-index element
// routing, the Annex B `__proto__` setter. None can apply to a symbol
// key, and each would read Str payload offsets off a 16-byte Symbol
// cell.
//
// What the two lanes DO share is the part that ignores how a key is
// spelled: §10.1.9.2 OrdinarySet's prototype-chain consult on an own
// miss — an inherited accessor writes through its setter with the
// original receiver, an inherited non-writable data property rejects.
// That walk was extracted so both lanes call one copy. An own hit
// needs nothing special: the dynobj set already dispatches an accessor
// entry's setter and refuses a non-writable data entry.

const s = Symbol("k");
const o: any = {};
o[s] = 1;
console.log("create", o[s], Object.getOwnPropertySymbols(o).length);
o[s] = 2;
console.log("overwrite", o[s], Object.getOwnPropertySymbols(o).length);
// the write is invisible to every string-key surface
console.log("names", JSON.stringify(Object.getOwnPropertyNames(o)));
console.log("keys", JSON.stringify(Object.keys(o)));
console.log("json", JSON.stringify(o));

// a different symbol with the same description is a different slot
const s2 = Symbol("k");
o[s2] = 3;
console.log("distinct", o[s], o[s2], Object.getOwnPropertySymbols(o).length);
o[Symbol.replace] = "R";
console.log("wk", o[Symbol.replace], Object.getOwnPropertySymbols(o).length);

// a heap payload round-trips
o[s] = { deep: [1, 2] };
console.log("heap", JSON.stringify(o[s]));
o[s] = "str";
console.log("str", o[s]);
o[s] = null;
console.log("null", o[s], Object.getOwnPropertySymbols(o).length);

// the write attributes are the ordinary-create defaults
const attrs: any = {};
attrs[s] = 1;
const d: any = Object.getOwnPropertyDescriptor(attrs, s);
console.log("attrs", d.value, d.writable, d.enumerable, d.configurable);

// receivers whose dict is an in-layout slot allocate it on first write
const fn: any = function g() {};
fn[s] = 5;
console.log("fn", fn[s], fn.name, fn.length);
const arr: any = [1, 2];
arr[s] = 6;
console.log("arr", arr[s], arr[0], arr.length, JSON.stringify(arr));

// §10.1.9.2 — an inherited non-writable data property rejects
const roProto: any = {};
Object.defineProperty(roProto, s2, { value: "ro", writable: false });
const roChild: any = Object.create(roProto);
try {
  roChild[s2] = "nope";
  console.log("ro", "no throw", roChild[s2]);
} catch (e: any) {
  console.log("ro", "threw", e instanceof TypeError);
}

// §10.1.9.2 — an inherited accessor writes through its setter, and no
// own entry is created on the child
let sink = "";
const accProto: any = {};
Object.defineProperty(accProto, s, {
  set: function (v: any) {
    sink = "set:" + v;
  },
  get: function () {
    return sink;
  },
});
const accChild: any = Object.create(accProto);
accChild[s] = 9;
console.log("accessor", sink, Object.getOwnPropertySymbols(accChild).length);

// string keys keep behaving on the same object
const mix: any = {};
mix["x"] = 1;
mix[s] = 2;
mix.y = 3;
console.log("mix", mix["x"], mix.y, mix[s]);
console.log("mix-keys", JSON.stringify(Object.keys(mix)), JSON.stringify(mix));
