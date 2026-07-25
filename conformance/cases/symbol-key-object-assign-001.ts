// `Object.assign` copies own ENUMERABLE symbol keys.
//
// §20.1.2.1 step 4.b.i runs OwnPropertyKeys, so the copy covers all
// three §10.1.11.1 buckets — indices, strings, and symbols — filtered
// on `[[Enumerable]]`. tr's assign kernel is shape-blind: it drives off
// the own-keys kernel, which answers the string buckets only (that
// split is what keeps `Object.keys` / `JSON.stringify` from leaking
// symbols). So the symbol bucket is a second pass over the same
// per-key body, from the enumerable-filtered symbol-keys kernel — the
// filter `getOwnPropertySymbols` deliberately does NOT apply.

const s = Symbol("visible");
const hidden = Symbol("hidden");

const src: any = { a: 1 };
src[s] = 2;
Object.defineProperty(src, hidden, { value: 3, enumerable: false });

const dst: any = Object.assign({}, src);
// the enumerable symbol key came across; the non-enumerable one did not
console.log("copied", dst.a, dst[s], dst[hidden]);
console.log("symbols", Object.getOwnPropertySymbols(dst).length);
console.log("keys", JSON.stringify(Object.keys(dst)));
console.log("json", JSON.stringify(dst));

// the copy is a real own property of the target, with the ordinary
// create attributes
const d: any = Object.getOwnPropertyDescriptor(dst, s);
console.log("desc", d.value, d.writable, d.enumerable, d.configurable);

// last source wins, same as for string keys
const first: any = {};
first[s] = "one";
const second: any = {};
second[s] = "two";
console.log("multi", Object.assign({}, first, second)[s]);
console.log("multi-rev", Object.assign({}, second, first)[s]);

// a symbol-keyed getter is invoked once, and its RESULT is copied (not
// the accessor)
let hits = 0;
const withGetter: any = {};
Object.defineProperty(withGetter, s, {
  get: function () {
    hits++;
    return "got";
  },
  enumerable: true,
  configurable: true,
});
const fromGetter: any = Object.assign({}, withGetter);
console.log("getter", fromGetter[s], hits);
const gd: any = Object.getOwnPropertyDescriptor(fromGetter, s);
console.log("getter-desc", gd.value, typeof gd.get);

// a nullish source contributes nothing (step 4.a), no throw
const target: any = {};
target[s] = "kept";
Object.assign(target, null, undefined);
console.log("nullish", target[s], Object.getOwnPropertySymbols(target).length);

// string-only sources are unaffected
console.log("strings-only", JSON.stringify(Object.assign({}, { x: 1 }, { y: 2 })));
