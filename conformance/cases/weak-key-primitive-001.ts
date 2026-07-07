// Chunk 630 — primitive weak keys classify as borrowed reads: the
// key box takes no refcount and an owned-shape key temp settles
// after classification (pre-fix each miss leaked the key string —
// probe l11: 300k `wm.has("key" + i)` churn 16.1MB vs 6.4MB flat).
// Behavioural surface: misses read absent, set/add on an illegal
// key is a catchable TypeError (ES CanBeHeldWeakly).
const wm = new WeakMap();
class K {
  v: number = 0;
}
const anchor = new K();
wm.set(anchor, 42);

// owned-temp string keys: has/get/delete read absent
let hits = 0;
for (let i = 0; i < 1000; i++) {
  const probe: string = "key" + i;
  if (wm.has(probe)) hits++;
}
console.log(hits);
console.log(wm.get("key1"));
console.log(wm.delete("key2"));
console.log(wm.get(anchor));

// illegal primitive key on set is a catchable TypeError
try {
  wm.set("nope", 1);
} catch (e) {
  console.log("set caught");
}

// WeakSet mirror
const ws = new WeakSet();
ws.add(anchor);
console.log(ws.has("s"));
try {
  ws.add("nope");
} catch (e) {
  console.log("add caught");
}
console.log(ws.has(anchor));
