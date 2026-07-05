// L3b #7 — for-of src temp ownership (typed tier): direct-form
// iterator / receiver temps are released at loop exit; borrow forms
// stay untouched and usable after the loop; break / return exits
// release through the after-block drop / owned-locals walk.
const m = new Map<number, string>();
m.set(1, "a");
m.set(2, "b");
m.set(3, "c");

// direct form: MapIter temps minted by the src expression
for (const k of m.keys()) console.log(k);
for (const v of m.values()) console.log(v);
for (const e of m.entries()) console.log(e[0], e[1]);

// the map survives its dead iterator temps
console.log(m.size);
console.log(m.get(2));

const s = new Set<number>();
s.add(10);
s.add(20);
for (const k of s.keys()) console.log(k);
console.log(s.size);

// borrow form: bound iterator is NOT dropped by the loop — a second
// for-of over the exhausted iter runs zero iterations, no release
const it2 = m.keys();
for (const k of it2) console.log(k);
for (const k of it2) console.log("again", k);
console.log("borrow ok");

// break exit releases the temp at the after-block
for (const k of m.keys()) {
  if (k === 2) break;
  console.log("b", k);
}

// return inside the body releases via the owned-locals walk
function f(mm: Map<number, string>): number {
  for (const k of mm.keys()) {
    if (k === 2) return k * 100;
  }
  return -1;
}
console.log(f(m));

// Map src minted by a user fn (Ident-callee call → owned temp)
function mk(): Map<number, number> {
  const t = new Map<number, number>();
  t.set(5, 50);
  t.set(6, 60);
  return t;
}
for (const e of mk()) console.log(e[0], e[1]);
console.log("done");
