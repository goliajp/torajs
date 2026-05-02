// Adapted from test262: built-ins/Object/assign. tr's subset MVP
// requires both args to share the same struct type and copies all
// fields from source to target in declaration order. Returns target
// so chained / let-bound assignment is well-typed. Source is borrowed
// (its refcounted fields stay alive — both target and source share
// each one after the call).
type Item = { name: string, count: number };
type Box = { items: Item[], total: number };

function check(): number {
  // Primitive + string fields.
  let target: Item = { name: "old", count: 1 };
  let source: Item = { name: "new", count: 99 };
  let r = Object.assign(target, source);
  if (target.name !== "new") { throw "#1: target.name"; }
  if (target.count !== 99) { throw "#2: target.count"; }
  if (r.name !== "new") { throw "#3: r.name"; }
  if (r.count !== 99) { throw "#4: r.count"; }
  // Source unchanged after the call (borrow, not consume).
  if (source.name !== "new") { throw "#5: source.name"; }
  if (source.count !== 99) { throw "#6: source.count"; }

  // Refcounted field: array. target's old array gets dropped; source's
  // array is deep-cloned (arr_slice + element rc_inc) so target ends
  // up with its own array referencing the same elements. Shared
  // elements between target's old items and source's items are safe
  // — array-literal lowering rc_inc's refcounted-borrow elements so
  // each appearance of `it1` holds its own ref.
  let it1: Item = { name: "apple", count: 5 };
  let it2: Item = { name: "banana", count: 3 };
  let target_box: Box = { items: [it1], total: 5 };
  let source_box: Box = { items: [it1, it2], total: 8 };
  Object.assign(target_box, source_box);
  if (target_box.items.length !== 2) { throw "#7"; }
  if (target_box.items[0].name !== "apple") { throw "#8"; }
  if (target_box.items[1].name !== "banana") { throw "#9"; }
  if (target_box.total !== 8) { throw "#10"; }
  if (source_box.items.length !== 2) { throw "#11"; }
  // it1 still alive after the assign + its source/target sharing.
  if (it1.name !== "apple") { throw "#11b: it1 corrupted"; }
  if (it1.count !== 5) { throw "#11c: it1 corrupted"; }

  // Self-assign should be a no-op observable-wise (drops + restores
  // each refcounted field).
  Object.assign(target, target);
  if (target.name !== "new") { throw "#12: self-assign clobbered"; }
  if (target.count !== 99) { throw "#13"; }
  return 0;
}
console.log(check());
