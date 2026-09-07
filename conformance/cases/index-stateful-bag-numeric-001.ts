// A numeric key on a shape that keeps an ordinary own-property bag is
// the stringified key (§7.1.19) landing in that bag — the same place
// the name-key channel already used. Read and write ask one table, so
// they cannot disagree about which shapes have a bag.

const m = new Map([["k", 1]]);
(m as any)[0] = 10;
console.log((m as any)[0]);
console.log(m.get("k"));
console.log(m.size);

const s = new Set([1]);
(s as any)[7] = 20;
console.log((s as any)[7]);
console.log(s.has(1));
console.log(s.size);

const r = /ab/g;
(r as any)[3] = 30;
console.log((r as any)[3]);
console.log(r.source);
console.log(r.lastIndex);

const it = [1, 2][Symbol.iterator]();
(it as any)[0] = 40;
console.log((it as any)[0]);
console.log(it.next().value);

// a miss in the bag is still undefined, not a throw
console.log((m as any)[99]);

// the name channel keeps answering the same way it did
(m as any).z = 50;
console.log((m as any).z);
