// r381 — a multi-argument generic inside a fn-type or inline-object
// annotation was cut in half. The parser's normal form spells both the
// marker's own separator and a generic's argument separator `|`
// (`(m: Map<string, number>) => void` → `__fn(Map<string|number>)->void`),
// so the splitter has to nest `<..>`. Four hand-rolled copies of that
// splitter existed and only one nested angles; the other three cut
// `Map<string|number>` into `Map<string` and `number>` and reported a
// loud "unknown type". Single-argument generics carry no `|`, which is
// why `Array<number>` always worked and this stayed hidden.

function takeMap(m: Map<string, number>) {
  console.log("fn-type param", m.get("k"), m.size);
}
const viaFnType: (m: Map<string, number>) => void = takeMap;
viaFnType(new Map<string, number>([["k", 9]]));

// return position
const makeMap: () => Map<string, number> = () => new Map<string, number>([["r", 1]]);
console.log("fn-type ret", makeMap().get("r"));

// inline object type, plain and nested
const holder: { m: Map<string, number> } = { m: new Map<string, number>([["h", 2]]) };
console.log("inlobj", holder.m.get("h"));

const deep: { inner: { m: Map<string, number>; n: number } } = {
  inner: { m: new Map<string, number>([["d", 3]]), n: 7 },
};
console.log("inlobj nested", deep.inner.m.get("d"), deep.inner.n);

// two fields, the generic one first — the miscut used to swallow the
// rest of the field list
const two: { m: Map<string, number>; tag: string } = {
  m: new Map<string, number>([["t", 4]]),
  tag: "ok",
};
console.log("two fields", two.m.get("t"), two.tag);

// the return arrow's `>` is still not a generic closer (the one case
// the angle-aware copy already handled — kept as a guard)
const arrowInGeneric: { f: (n: number) => number; s: string } = {
  f: (n: number) => n * 2,
  s: "guard",
};
console.log("arrow guard", arrowInGeneric.f(21), arrowInGeneric.s);

// a generic argument that is itself a generic
const nestedArg: (m: Map<string, Array<number>>) => void = (m) => {
  console.log("nested generic arg", m.get("a")![1]);
};
nestedArg(new Map<string, Array<number>>([["a", [10, 20]]]));
