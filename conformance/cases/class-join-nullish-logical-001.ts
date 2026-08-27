// rotation 507 — `??` and `||` / `&&` join two different class
// instances at their nearest common ancestor, the same rule the ternary
// takes (507-01); both used to reject where bun runs the program. The
// joined value stays an unboxed pointer on the ancestor's repr, so a
// method call on it dispatches through the vtable on the static
// ancestor. Shapes: nullable-lhs `??` with a deeper rhs, the reverse
// (deeper lhs, shallower rhs — the slot must widen to the ancestor),
// `||` through a param, `&&` over two siblings, no common ancestor
// (joins to any), and the join flowing into a base-typed binding, a
// return, and an array element.
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
}
class Other extends Base {
  name(): string {
    return "other";
  }
  score(): number {
    return -this.id;
  }
}
class Alone {
  tag: string;
  constructor(tag: string) {
    this.tag = tag;
  }
  name(): string {
    return "alone:" + this.tag;
  }
}
function orDefault(m: Mid | null): Base {
  return m || new Leaf(2);
}
function nullishDefault(l: Leaf | null): Base {
  return l ?? new Mid(3);
}
const a: Mid | null = null;
const j1 = a ?? new Leaf(1);
console.log(j1.name(), j1.score(), j1.id);
const b: Mid | null = new Mid(9);
const j2 = b ?? new Leaf(1);
console.log(j2.name(), j2.score());
// deeper lhs, shallower rhs — the ancestor is the rhs's own class
const c: Leaf | null = null;
const j3 = c ?? new Mid(4);
console.log(j3.name(), j3.score());
console.log(orDefault(null).name(), orDefault(new Mid(5)).name());
console.log(nullishDefault(null).name(), nullishDefault(new Leaf(6)).name());
// two siblings under one root
const d: Other | null = null;
const j4 = d ?? new Leaf(7);
console.log(j4.name(), j4.score());
// no common ancestor → any
const e: Mid | null = null;
const j5 = e ?? new Alone("z");
console.log(j5.name());
const arr: Base[] = [a ?? new Leaf(11), b ?? new Other(12)];
let total = 0;
for (const x of arr) {
  console.log(x.name());
  total += x.score();
}
console.log(total);
const g: Mid | null = new Leaf(20);
const j6 = g && new Other(21);
console.log(j6 ? j6.name() : "falsy");
