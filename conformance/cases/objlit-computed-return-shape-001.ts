// A computed key has no static name, so the inline-object return
// annotation built from the field names spelled the parser's
// `__computed_<n>__` sentinel into the caller's view of the shape.
// The value itself lowers through the dynobj lane, so reads and
// `Object.keys` answered right while `JSON.stringify` serialized the
// sentinel — and the wrong value under it.
function mk(k: string, v: number) {
  return { [k]: v, z: 0 };
}

const r = mk("a", 1) as any;
console.log(JSON.stringify(r));
console.log(r["a"], r.z);
console.log(Object.keys(r).join(","));

// Two calls with different keys share one inferred shape; neither may
// borrow the other's.
console.log(JSON.stringify(mk("a", 5)), JSON.stringify(mk("b", 6)));

// A computed key that comes second keeps its position.
function trailing(k: string) {
  return { z: 0, [k]: 1 };
}
console.log(JSON.stringify(trailing("a")));

// The arrow spelling reaches the same inference.
const arrow = (k: string) => ({ [k]: 1, z: 0 });
console.log(JSON.stringify(arrow("a")));

// A literal that merely CONTAINS one: the outer shape stays static,
// the inner one does not.
function nested(k: string) {
  return { wrap: { [k]: 1, z: 0 } };
}
console.log(JSON.stringify(nested("a")));

// Reading a member the object does not have is `undefined`, not a
// type error naming a sentinel.
console.log(mk("a", 1).nope);
