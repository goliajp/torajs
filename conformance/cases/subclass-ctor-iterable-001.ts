// builtin-subclass ctor — new C(iterable) hands the argument to the
// builtin [[Construct]] (§24.2.1.1 / §24.1.1.1); the ctor-less class
// gets the derived default ctor's forward.
class MySet extends Set {
  has(v: any) { return super.has(v); }
}
const s = new MySet([1, 2]);
console.log([...s].join(","), s.size);
const empty = new MySet();
console.log(empty.size);
class MyMap extends Map {
}
const m = new MyMap([["a", 1], ["b", 2]]);
console.log(m.size, m.get("b"));
const mn = new MyMap(null);
console.log(mn.size);
// explicit ctor forwarding one arg keeps working
class Pre extends Set {
  constructor(iter: any) { super(iter); this.add(99); }
}
const p = new Pre([7]);
console.log([...p].join(","));
console.log("done");
