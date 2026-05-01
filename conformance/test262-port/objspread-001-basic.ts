// Adapted from test262: language/expressions/object/* — object spread
// `{ ...src, k: v }`. tr static-shape: typecheck unfolds source
// struct's fields into the inferred layout (later inline members
// replace earlier slots on key collision); ssa-lower copies each
// source field offset-by-offset.
type Pt = { x: number, y: number };
type PtZ = { x: number, y: number, z: number };
type Pt2 = { x: number, y: number };  // override scenario
type Named = { name: string, age: number };
type NamedX = { name: string, age: number, role: string };

function check(): number {
  // Add a field via spread.
  let p: Pt = { x: 5, y: 7 };
  let q: PtZ = { ...p, z: 9 };
  if (q.x !== 5) { throw "#1"; }
  if (q.y !== 7) { throw "#2"; }
  if (q.z !== 9) { throw "#3"; }

  // Override: inline member replaces spread source's value.
  let p_orig: Pt = { x: 1, y: 2 };
  let p_overridden: Pt2 = { ...p_orig, x: 100 };
  if (p_overridden.x !== 100) { throw "#4: inline overrides"; }
  if (p_overridden.y !== 2) { throw "#5: untouched"; }

  // Original source unchanged (spread copies values).
  if (p_orig.x !== 1) { throw "#6: source unchanged"; }
  if (p_orig.y !== 2) { throw "#7"; }

  // String fields too.
  let alice: Named = { name: "alice", age: 30 };
  let alice_role: NamedX = { ...alice, role: "admin" };
  if (alice_role.name !== "alice") { throw "#8"; }
  if (alice_role.age !== 30) { throw "#9"; }
  if (alice_role.role !== "admin") { throw "#10"; }
  return 0;
}
console.log(check());
