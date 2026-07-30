// class C extends Map | Set — exotic-backed collection instances
// (RFC 20260730 blade 2). The instance is a REAL Map/Set cell (the
// whole set/get/has/add/size surface rides the existing arms); class
// identity rides FLAG_SUBCLASSED + the blade-0 side table.

// 1. Map subclass — builtin faces
class MyMap extends Map {}
const m = new MyMap();
console.log(m instanceof Map, m instanceof MyMap);
console.log(Object.getPrototypeOf(m) === MyMap.prototype);
m.set("a", 1);
m.set("b", 2);
console.log(m.get("a"), m.size, m.has("b"), m.has("z"));

// 2. class methods over the exotic receiver (builtins riding inside)
class Counter extends Map {
  bump(k: string): number {
    const cur = this.get(k);
    const next = (cur === undefined ? 0 : cur) + 1;
    this.set(k, next);
    return next;
  }
}
const c = new Counter();
c.bump("x");
c.bump("x");
console.log(c.get("x"), c.size);

// 3. Set subclass
class MySet extends Set {}
const s = new MySet();
s.add(5);
s.add(5);
s.add(7);
console.log(s.has(5), s.size, s instanceof Set, s instanceof MySet);
s.delete(5);
console.log(s.has(5), s.size);

// 4. plain collections keep their answers
console.log(new Map() instanceof MyMap, new Set() instanceof MySet);

// 5. override wins over the builtin name
class Loud extends Set {
  has(v: any): boolean {
    return true;
  }
}
const l = new Loud();
console.log(l.has(999));
