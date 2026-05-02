// Refcounted Obj field shared across multiple struct literals. Earlier
// the typechecker rejected the second use of `it1` as "already moved",
// since object-literal field init was using move semantics. Now non-
// Copy fields rc_inc at lower time so a single refcounted heap object
// can be the value of fields in multiple structs simultaneously.
type Item = { name: string, count: number };
type Wrap = { it: Item, kind: string };
type Group = { items: number[], total: number };

function check(): number {
  let it1: Item = { name: "apple", count: 5 };
  let w1: Wrap = { it: it1, kind: "first" };
  let w2: Wrap = { it: it1, kind: "second" };

  if (w1.it.name !== "apple") { throw "#1"; }
  if (w2.it.name !== "apple") { throw "#2"; }
  if (w1.it.count !== 5) { throw "#3"; }
  if (w2.it.count !== 5) { throw "#4"; }
  if (it1.name !== "apple") { throw "#5: it1 corrupted"; }
  if (it1.count !== 5) { throw "#6"; }

  // Two structs sharing a refcounted Array field.
  let xs: number[] = [10, 20, 30];
  let g1: Group = { items: xs, total: 60 };
  let g2: Group = { items: xs, total: 90 };
  if (g1.items.length !== 3) { throw "#7"; }
  if (g2.items.length !== 3) { throw "#8"; }
  if (g1.items[0] !== 10) { throw "#9"; }
  if (g2.items[2] !== 30) { throw "#10"; }
  if (xs.length !== 3) { throw "#11: xs corrupted"; }
  return 0;
}
console.log(check());
