// A shape whose entire ordinary own face is its property bag
// enumerates exactly that bag. Which shapes those are is one
// predicate, not a list repeated per surface — a shape admitted to
// the bag but missing from an enumeration list reads back a property
// that for-in cannot see.

function keys(o: any): string {
  const out: string[] = [];
  for (const k in o) { out.push(k); }
  return out.join(",");
}

const m = new Map([["entry", 1]]);
(m as any).a = 1;
(m as any)[0] = 2;
console.log(keys(m));
console.log(m.size);

const s = new Set([1]);
(s as any).b = 1;
console.log(keys(s));

const d = new Date(0);
(d as any).c = 1;
console.log(keys(d));

const r = /x/g;
(r as any).e = 1;
console.log(keys(r));
console.log(r.lastIndex);

const wm = new WeakMap();
(wm as any).a = 1;
(wm as any)[0] = 2;
console.log(keys(wm));

const ws = new WeakSet();
(ws as any).f = 1;
console.log(keys(ws));

const wr = new WeakRef({ id: 1 });
(wr as any).g = 1;
console.log(keys(wr));

const it = [1, 2][Symbol.iterator]();
(it as any).h = 1;
console.log(keys(it));
