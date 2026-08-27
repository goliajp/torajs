// rotation 507 — a method name that is overridden inside one hierarchy
// AND declared by an unrelated class used to lose its virtual dispatch:
// the vtable slot set only admitted single-chain names, so every
// `(b: Base = new Leaf()).name()` ran Base's body once `Alone.name()`
// existed. Shapes: unrelated declarer, two siblings introducing a name
// below a silent base (a derived-typed binding wearing a grandchild),
// a throwing override reached through the base type (the throw must
// propagate), and the unrelated class keeping its own body.
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
    if (this.id === 13) throw new Error("thirteen");
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
  score(): number {
    return 7;
  }
}
// two siblings introduce `label` below a base that never declares it
class Root {
  k: number;
  constructor(k: number) {
    this.k = k;
  }
}
class L1 extends Root {
  label(): string {
    return "l1";
  }
}
class R1 extends Root {
  label(): string {
    return "r1";
  }
}
class L2 extends L1 {
  label(): string {
    return "l2:" + this.k;
  }
}
function viaParam(b: Base): string {
  return b.name() + "/" + b.score();
}
const b: Base = new Leaf(1);
console.log(b.name(), b.score());
const c: Base = new Other(2);
console.log(c.name(), c.score());
console.log(viaParam(new Other(3)), viaParam(new Leaf(4)), viaParam(new Mid(5)), viaParam(new Base(6)));
const m: Mid = new Leaf(8);
console.log(m.name(), m.score());
const a = new Alone("z");
console.log(a.name(), a.score());
const all: Base[] = [new Base(1), new Mid(2), new Leaf(3), new Other(4)];
let total = 0;
for (const x of all) {
  console.log(x.name());
  total += x.score();
}
console.log(total);
try {
  const bad: Base = new Other(13);
  console.log(bad.name());
  console.log(bad.score());
  console.log("unreachable");
} catch (e) {
  console.log("caught", (e as Error).message);
}
const l: L1 = new L2(9);
console.log(l.label());
const r: Root = new R1(3);
const l1: L1 = new L1(4);
console.log(l1.label(), r.k);
const arr: L1[] = [new L1(1), new L2(2)];
for (const x of arr) console.log(x.label());
