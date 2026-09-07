// The same receiver, two spellings: a binding whose static type is Map
// and a binding widened to any. Both reach the same runtime walk, so
// both must answer the same thing — the typed spelling used to die at
// "Object.keys requires a struct arg, got Map" while the widened one
// was already correct.

const m = new Map([["e", 1]]);
(m as any).a = 1;
(m as any)[0] = 2;
console.log(Object.keys(m as any).join(","));
console.log(Object.values(m as any).join(","));
console.log(Object.entries(m as any).length);
console.log(m.size);

const widened: any = m;
console.log(Object.keys(widened).join(","));
console.log(Object.values(widened).join(","));

const d = new Date(0);
(d as any).z = 1;
console.log(Object.keys(d as any).join(","));
console.log(d.getTime());

const r = /x/g;
(r as any).y = 2;
console.log(Object.keys(r as any).join(","));
console.log(r.lastIndex);

const wm = new WeakMap();
(wm as any).w = 3;
console.log(Object.keys(wm as any).join(","));
console.log(Object.values(wm as any).join(","));

// a struct still takes the compile-time layout path
const o = { p: 1, q: 2 };
console.log(Object.keys(o).join(","));
console.log(Object.values(o).join(","));
