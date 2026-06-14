// Builtin container generics `Map<K, V>` / `Set<T>` / `WeakMap<K, V>` /
// `WeakSet<T>` accept K / V annotations as erased type-system args in
// fn-param / let-decl positions. Prior to this fixture the parser's flat
// ann encoding `Map<K|V>` was rejected by `resolve_type_ann_full` (only
// the bare `Map` arm matched), so every user pattern that annotated a
// Map / Set in a fn signature or a let-binding fell through to "unknown
// type `Map<K|V>`" — covering the realistic surface (sig types are how
// users actually wire typed containers across module boundaries).
//
// Fixture stays narrow to the ann-resolution surface. Class-field-typed
// containers + WeakMap with `object` keys are independent pre-existing
// wedges (class-field init falls into ctor-body inference; `object` type
// ann is not yet a resolver entry) and are covered separately when
// those surfaces consume.

function describeMap(m: Map<number, string>): number {
  return m.size;
}
const fm = new Map<number, string>();
fm.set(7, "seven");
fm.set(13, "thirteen");
console.log(describeMap(fm));
console.log(fm.get(7));

function hasId(s: Set<number>, id: number): boolean {
  return s.has(id);
}
const fs = new Set<number>();
fs.add(10);
fs.add(20);
fs.add(10);
console.log(hasId(fs, 10));
console.log(hasId(fs, 99));
console.log(fs.size);

const lm: Map<number, string> = new Map<number, string>();
lm.set(5, "five");
console.log(lm.get(5));
console.log(lm.size);

const ls: Set<number> = new Set<number>();
ls.add(1);
ls.add(2);
ls.add(2);
console.log(ls.size);
