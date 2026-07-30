// RFC 20260730-iterator-global 刀 4 长尾 — @@iterator return-this
// (§27.1.2.1): iterator cells answer themselves, the read reifies a
// function, and the generic consumers (spread, Array.from) ride it.

function* g() {
  yield 1;
  yield 2;
}

const it: any = [10, 20].values();
console.log(typeof it[Symbol.iterator]);
const it2: any = [30].values();
console.log(it2[Symbol.iterator]() === it2);
console.log([...([40, 50].values() as any)]);

const h: any = ([1, 2].values() as any).map((v: any) => v);
console.log(typeof h[Symbol.iterator]);
console.log([...h]);

const m = new Map();
m.set("k", 9);
const me: any = m.entries();
console.log(me[Symbol.iterator]() === me);

console.log(Array.from([7, 8].values() as any));

// A generator instance keeps its own return-this (§27.5.1.1 lane).
const gi: any = g();
console.log(gi[Symbol.iterator]() === gi);
console.log([...(g() as any)]);
