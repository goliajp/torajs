// rotation 507 (506-02) — a ternary over two DIFFERENT class instances
// joins to their nearest common ancestor instead of being rejected
// (`ternary branches differ`); the value stays an unboxed pointer on
// the ancestor's repr and every method call dispatches on the static
// ancestor through the vtable. Shapes: siblings (LCA = shared parent),
// parent × grandchild (LCA = the parent itself), cousins across two
// subtrees (LCA = the root), no common ancestor (joins to any), and
// the join flowing into a declared base binding, a return, a param,
// and an array element.
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
function viaParam(b: Base): string {
  return b.name() + "/" + b.score();
}
function pick(f: boolean, i: number): Base {
  return f ? new Mid(i) : new Other(i);
}
let total = 0;
for (let i = 0; i < 6; i++) {
  const sib = i % 2 === 0 ? new Mid(i) : new Leaf(i);
  console.log(sib.name(), sib.score());
  const up = i % 3 === 0 ? new Base(i) : new Leaf(i);
  console.log(up.name(), up.score(), up.id);
  const cousin: Base = i < 3 ? new Other(i) : new Leaf(i);
  console.log(cousin.name(), cousin.score());
  total += sib.score() + up.score() + cousin.score();
  console.log(viaParam(i % 2 === 1 ? new Other(i) : new Mid(i)));
  const none = i % 2 === 0 ? new Alone("a" + i) : new Leaf(i);
  console.log(none.name());
}
console.log(total);
console.log(pick(true, 7).name(), pick(false, 7).name(), pick(false, 7).score());
const arr: Base[] = [true ? new Leaf(1) : new Other(2), false ? new Leaf(3) : new Mid(4)];
for (const b of arr) console.log(b.name(), b.score());
