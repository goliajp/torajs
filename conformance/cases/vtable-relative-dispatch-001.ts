// r506 — vtable slots are RELATIVE (`fn - table`, added back at the
// dispatch site) so the table needs no dyld fixup and lives in
// `__TEXT`; a class-override program no longer carries a
// `__DATA_CONST` page. Every polymorphic shape the slot arithmetic
// has to survive: method index > 0 (non-zero slot offset), a
// three-level chain where the middle class overrides one method and
// inherits the other (slot resolves to an ancestor impl at a
// different distance from the table), a leaf that overrides both,
// receivers reached through a base-typed array, a base-typed
// parameter, and a base-typed local; plus a method declared only on
// the leaf (its base-table slot is empty and never loaded).
class Base {
  id: number;
  constructor(id: number) {
    this.id = id;
  }
  name(): string {
    return "base";
  }
  score(): number {
    return this.id;
  }
  describe(): string {
    return this.name() + ":" + this.score();
  }
}
class Mid extends Base {
  name(): string {
    return "mid";
  }
}
class Leaf extends Mid {
  name(): string {
    return "leaf";
  }
  score(): number {
    return this.id * 100;
  }
  extra(): string {
    return "only-leaf";
  }
}
function viaParam(b: Base): string {
  return b.describe();
}
const all: Base[] = [new Base(1), new Mid(2), new Leaf(3)];
for (const b of all) {
  console.log(b.name(), b.score(), b.describe());
}
console.log(viaParam(new Leaf(4)), viaParam(new Mid(5)));
const asBase: Base = new Leaf(6);
console.log(asBase.name(), asBase.score());
const leaf = new Leaf(7);
console.log(leaf.extra(), leaf.describe());
let total = 0;
for (let i = 0; i < 1000; i++) {
  let b: Base = new Base(i);
  if (i % 3 === 1) {
    b = new Mid(i);
  } else if (i % 3 === 2) {
    b = new Leaf(i);
  }
  total += b.score();
}
console.log(total);
