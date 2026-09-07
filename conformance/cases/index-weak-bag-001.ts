// §24.3 / §24.4 / §26.1 — a weak collection's entry table and a
// WeakRef's observed target are internal state watched by the
// registry, never properties. So their whole ordinary own face is a
// property bag, exactly like Map's, and both keyed spellings reach it.

const k = { id: 1 };

const wm = new WeakMap();
wm.set(k, "v");
(wm as any).z = 4;
(wm as any)[0] = 8;
console.log((wm as any).z);
console.log((wm as any)[0]);
console.log((wm as any)[99]);
// the entry table is untouched by the bag
console.log(wm.get(k));
console.log(wm.has(k));

const ws = new WeakSet();
ws.add(k);
(ws as any).tag = "s";
(ws as any)[1] = 5;
console.log((ws as any).tag);
console.log((ws as any)[1]);
console.log(ws.has(k));

const wr = new WeakRef(k);
(wr as any).note = "r";
(wr as any)[2] = 6;
console.log((wr as any).note);
console.log((wr as any)[2]);
console.log(wr.deref().id);
