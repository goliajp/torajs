// TS `object` type annotation — the non-primitive heap-shape per
// TS spec §3.2.8 (any value except null / undefined / number / string /
// boolean / bigint / symbol). Prior to this fixture the parser flat
// ann string `object` fell through `resolve_type_ann_inner` /
// `parse_type` and reported "unknown type `object`" — breaking the
// common `WeakMap<object, V>` key signature and any `function f(o:
// object)` user pattern.
//
// The subset collapses `object` to `Type::Any` since the non-primitive
// narrowing constraint (e.g. rejecting `let x: object = 5`) is
// independent substrate work; this fixture covers the ann-resolution
// surface only. Independent L3b: enforce the constraint at assign-time.

function describe(o: object): number {
  return 1;
}
console.log(describe({}));
console.log(describe({ a: 1 }));

const o: object = { x: 42 };
console.log(typeof o);

function holder(o: object, k: string): string {
  return "held";
}
const arg: object = { id: "abc" };
console.log(holder(arg, "k"));
