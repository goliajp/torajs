// any-method-call RFC C4+ — Map/Set keys()/values()/entries()
// iterator mints + the Tag::MapIter next() IteratorResult surface.
const m: any = new Map();
m.set("a", 1);
m.set("b", 2);
const ki: any = m.keys();
let r: any = ki.next();
console.log(r.value);
console.log(r.done);
r = ki.next();
console.log(r.value);
console.log(r.done);
r = ki.next();
console.log(r.value);
console.log(r.done);
// exhausted iterators stay done forever
console.log(ki.next().done);
// values
const vi: any = m.values();
console.log(vi.next().value);
console.log(vi.next().value);
// entries → [k, v] pair
const ei: any = m.entries();
const e0: any = ei.next().value;
console.log(e0[0]);
console.log(e0[1]);
// Set: keys === values (elements), entries → [e, e]
const s: any = new Set();
s.add(10);
s.add(20);
console.log(s.keys().next().value);
console.log(s.values().next().value);
const se: any = s.entries().next().value;
console.log(se[0]);
console.log(se[1]);
// live iteration — entries added mid-walk are visited
const m2: any = new Map();
m2.set("x", 1);
const ki2: any = m2.keys();
console.log(ki2.next().value);
m2.set("y", 2);
console.log(ki2.next().value);
console.log(ki2.next().done);
// unknown method on an iterator throws
try {
  ki.foo();
} catch (err) {
  console.log("threw");
}
