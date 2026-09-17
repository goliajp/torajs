const w = new WeakRef({ a: 1 });
console.log("sync", (w.deref() as any)?.a);
class Box { v = 3; }
const w3 = new WeakRef(new Box());
console.log("box", (w3.deref() as any)?.v);
try { new WeakRef(1 as any); } catch (e) { console.log("te", e instanceof TypeError); }
