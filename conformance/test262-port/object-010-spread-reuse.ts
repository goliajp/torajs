// Object spread previously consumed the source (marked it moved) when
// any field was non-Copy, leaking the source's container alloc and
// blocking source reuse. Now spread per-field rc_inc's refcounted
// values into the new struct's slots and leaves the source live for
// scope-end drop / further reuse.
type Item = { name: string, count: number };
type Wrap = { it: Item, kind: string };

function check(): number {
  let it1: Item = { name: "apple", count: 5 };
  let base: Wrap = { it: it1, kind: "base" };
  let derived: Wrap = { ...base, kind: "derived" };
  let third: Wrap = { ...base, kind: "third" };

  if (base.kind !== "base") { throw "#1: base"; }
  if (derived.kind !== "derived") { throw "#2: derived"; }
  if (third.kind !== "third") { throw "#3: third"; }
  if (base.it.name !== "apple") { throw "#4"; }
  if (derived.it.name !== "apple") { throw "#5"; }
  if (third.it.name !== "apple") { throw "#6"; }
  if (it1.count !== 5) { throw "#7: it1 corrupted"; }
  if (base.it.count !== 5) { throw "#8"; }

  // Spread + override + reuse the source: source still readable.
  let bumped: Wrap = { ...base, it: { name: "banana", count: 9 } };
  if (bumped.it.name !== "banana") { throw "#9"; }
  if (base.it.name !== "apple") { throw "#10: base.it lost"; }
  return 0;
}
console.log(check());
