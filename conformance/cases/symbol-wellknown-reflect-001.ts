// Symbol well-known data statics reflection (§6.1.5.1) — 13
// string-named own DATA props on the Symbol ctor, all
// { writable: false, enumerable: false, configurable: false },
// value = the process-lifetime singleton (identity stable across
// reads); module-strict assign / delete both throw.
const S: any = Symbol;
const names = [
  "asyncDispose", "asyncIterator", "dispose", "hasInstance",
  "isConcatSpreadable", "iterator", "match", "matchAll", "replace",
  "search", "species", "split", "toPrimitive", "toStringTag",
  "unscopables",
];
for (const k of names) {
  const d = Object.getOwnPropertyDescriptor(S, k);
  console.log(k, typeof d.value, d.writable, d.enumerable, d.configurable,
    S.hasOwnProperty(k), Object.hasOwn(S, k), d.value === S[k], String(d.value));
}
// typed-lane direct reads (compile-time lowering, not the any-lane)
console.log(typeof Symbol.matchAll, typeof Symbol.hasInstance, typeof Symbol.unscopables);
console.log(Symbol.toStringTag === S.toStringTag, String(Symbol.species));
try { S.iterator = 1; } catch (e: any) { console.log("w:", e instanceof TypeError); }
try { delete S.iterator; } catch (e: any) { console.log("d:", e instanceof TypeError); }
console.log(typeof S.iterator, S.iterator === S.iterator);
