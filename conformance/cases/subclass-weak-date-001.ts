// Rotation 373 — WeakMap / WeakSet / Date builtin heritage (extends
// RFC 20260730-exotic-backed-class-instance blade 2): the subclass
// factory mints a REAL weak-collection / Date cell, the whole builtin
// surface rides the existing arms, `super(iterable)` rides the same
// §24.3.1.1 / §24.4.1.1 kernel the plain ctors use, and Date's
// `super(v)` runs the §21.4.2.1 step-4 value ladder onto the minted
// cell (mint default = current wall clock, so a bare `super()` is the
// no-argument `new Date()`). A ctor-less Date subclass stays a LOUD
// compile reject (argument-count-aware forward needs the real-argc
// face — L3b 372-00/02).

// 1. ctor-less WeakMap subclass: identity + full surface
class W extends WeakMap {}
const w = new W();
const k1 = {};
const k2 = {};
w.set(k1, 42);
console.log("wm", w.get(k1), w.has(k2), w instanceof W, w instanceof WeakMap);
console.log("wm-del", w.delete(k1), w.has(k1));

// 2. explicit ctor forwarding the §24.3.1.1 iterable
class WPairs extends WeakMap {
  constructor(entries: any) {
    super(entries);
  }
}
const wp = new WPairs([
  [k1, "a"],
  [k2, "b"],
]);
console.log("wm-iter", wp.get(k1), wp.get(k2));

// 3. `new W()` zero-arg through the synthesized forward: nullish no-op
const wEmpty = new W();
console.log("wm-empty", wEmpty.has(k1));

// 4. WeakSet subclass: ctor-less + iterable ctor
class S extends WeakSet {}
const s = new S();
s.add(k1);
console.log("ws", s.has(k1), s.has(k2), s instanceof S, s instanceof WeakSet);
class SPairs extends WeakSet {
  constructor(vals: any) {
    super(vals);
  }
}
const sp = new SPairs([k1, k2]);
console.log("ws-iter", sp.has(k1), sp.has(k2));

// 5. Date subclass: ms / string / Date-instance / bare super()
class D extends Date {
  constructor() {
    super();
  }
}
class D1 extends Date {
  constructor(v: any) {
    super(v);
  }
}
const dMs = new D1(86400000);
console.log("date-ms", dMs.getTime(), dMs.getUTCDate(), dMs instanceof D1, dMs instanceof Date);
const dStr = new D1("1970-01-05T00:00:00Z");
console.log("date-str", dStr.getUTCDate());
const dCopy = new D1(new Date(1234));
console.log("date-copy", dCopy.getTime());
const dNow = new D();
console.log("date-now", dNow.getFullYear() === new Date().getFullYear());

// 6. an invalid value lands as Invalid Date, same as the plain ctor
const dBad = new D1("not a date");
console.log("date-bad", Number.isNaN(dBad.getTime()));

// 7. explicit ctor with NO super(): §9.2.2 this-TDZ ReferenceError
class DNoSuper extends Date {
  constructor() {}
}
try {
  new DNoSuper();
  console.log("nosuper no throw");
} catch (e) {
  console.log("nosuper", (e as Error).constructor.name);
}

// 8. methods declared ON the subclass dispatch alongside the builtin
class Tagged extends WeakSet {
  label() {
    return "tagged";
  }
}
const t = new Tagged();
t.add(k2);
console.log("own-method", t.label(), t.has(k2));
