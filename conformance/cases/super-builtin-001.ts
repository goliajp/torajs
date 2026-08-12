// builtin-heritage super.m() — §13.3.7.3 resolves on the parent
// prototype, skipping the receiver's own override.
class MySet extends Set {
  has(v: any) { return super.has(v); }
  hasTwice(v: any) { return super.has(v) && this.has(v); }
}
const s = new MySet();
s.add(1);
s.add(2);
console.log(s.has(1), s.has(3));
console.log(s.hasTwice(2));
console.log([...s].join(","));

// counting override — super must not bounce back through it
let calls = 0;
class CountSet extends Set {
  has(v: any) { calls++; return super.has(v); }
}
const c = new CountSet();
c.add(9);
console.log(c.has(9), calls);

class MyMap extends Map {
  get(k: any) { const v = super.get(k); return v === undefined ? "dflt" : v; }
}
const m = new MyMap();
m.set("a", 1);
console.log(m.get("a"), m.get("zz"));

// a super name the builtin prototype lacks throws when called
class Bad extends Set {
  // @ts-expect-error — deliberately calling a name the parent lacks
  poke() { return super.nope(); }
}
const b = new Bad();
try { b.poke(); console.log("no-throw"); } catch (e) { console.log("nope:", (e as Error).name); }
console.log("done");
