// generic struct alias instantiation site: an ObjectLit missing an
// optional field fills `field: undefined` (fill_optional_fields
// generic-alias arm), and same-shaped literals under different
// instantiations of one generic pin their declared layouts (the
// objlit declared-layout hint — Box<number> vs Box<string> both
// lower `{v: undefined-ptr, label: str}`).
type Box<T> = { v?: T; label: string };
const a: Box<number> = { v: undefined, label: "a" };
console.log(a.v === undefined, a.v, a.label);
const b: Box<number> = { v: 42, label: "b" };
console.log(b.v === undefined, b.v);
const c: Box<number> = { label: "c" };
console.log(c.v === undefined, c.v, c.label);

// second instantiation of the SAME generic — the declared-layout
// hint keeps the Str-slot repr for v even though the literal shape
// matches the Box<number> layout registered above
const s: Box<string> = { label: "s" };
console.log(s.v === undefined, s.label);

// string-slot optional through a generic param
type Tag<T> = { name: string; note?: T };
const t: Tag<string> = { name: "t" };
console.log(t.note === undefined, t.name);

// multi-param generic (flat `Pair<number|string>` ann spelling)
type Pair<A, B> = { fst: A; snd?: B; tag: string };
const p: Pair<number, string> = { fst: 1, tag: "p" };
console.log(p.snd === undefined, p.fst, p.tag);

// fn-body site (recursive stmt walk)
function mk(): Box<string> {
  const inner: Box<string> = { label: "in" };
  return inner;
}
console.log(mk().v === undefined, mk().label);
