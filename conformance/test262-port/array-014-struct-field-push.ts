// Adapted from test262: built-ins/Array/prototype/push/* — `b.items.push(v)`
// where the array lives inside a struct field. tr previously only special-
// cased the `Ident` receiver shape; struct-field receivers must do the same
// load → push → store-back dance, but using the field's offset instead of a
// local slot.
type Bag = { items: number[], tag: string };

function check(): number {
  let b: Bag = { items: [10, 20], tag: "a" };
  b.items.push(30);
  b.items.push(40);
  if (b.items.length !== 4) { throw "#1"; }
  if (b.items[0] !== 10) { throw "#2"; }
  if (b.items[2] !== 30) { throw "#3"; }
  if (b.items[3] !== 40) { throw "#4"; }
  if (b.tag !== "a") { throw "#5"; }

  // Push enough to force a realloc — verify the field still points at
  // the new (live) buffer afterwards, not the freed one.
  for (let i = 0; i < 100; i = i + 1) { b.items.push(i); }
  if (b.items.length !== 104) { throw "#6"; }
  if (b.items[103] !== 99) { throw "#7"; }
  return 0;
}
console.log(check());
