// chunk 615 — CanBeHeldWeakly for symbols: a Symbol.for-registered
// symbol is an illegal WeakMap/WeakSet key (set/add throw TypeError,
// has/get/delete read it as absent); an unregistered symbol is legal.
const wm = new WeakMap();
const ws = new WeakSet();
const reg = Symbol.for("t");
const plain = Symbol("p");
wm.set(plain, 1);
console.log(wm.get(plain));
console.log(wm.has(plain));
ws.add(plain);
console.log(ws.has(plain));
try {
  wm.set(reg, 2);
} catch (e) {
  console.log("wm-set:", (e as Error).name);
}
try {
  ws.add(reg);
} catch (e) {
  console.log("ws-add:", (e as Error).name);
}
console.log(wm.has(reg));
console.log(wm.get(reg));
console.log(ws.has(reg));
console.log(wm.delete(reg));
