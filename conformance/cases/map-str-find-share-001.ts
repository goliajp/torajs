// The has / get / set counting idiom over string keys, where the code
// between the lookup and the write may rewrite the Map: a valueOf that
// deletes, clears and re-inserts keys must not let the set land on a
// stale entry.
const counts = new Map<string, number>();
const words = "a b c a b a d a".split(" ");
for (const w of words) {
  if (counts.has(w)) {
    counts.set(w, counts.get(w)! + 1);
  } else {
    counts.set(w, 1);
  }
}
console.log([...counts].join(";"));

const m = new Map<string, any>();
m.set("a", 1);
m.set("b", 2);
m.set("c", 3);
const bump = {
  valueOf(): number {
    m.delete("a");
    m.set("x", 7);
    m.set("a", 100);
    return 1;
  },
};
const k = "a";
if (m.has(k)) {
  m.set(k, m.get(k) + bump);
}
console.log([...m].join(";"));

const wipe = {
  valueOf(): number {
    m.clear();
    for (let i = 0; i < 40; i = i + 1) m.set("k" + i, i);
    return 2;
  },
};
const key = "b";
m.set(key, 5);
if (m.has(key)) {
  m.set(key, m.get(key) + wipe);
}
console.log(m.size, m.get("b"), m.get("k39"), m.get("a"));

let hits = 0;
const s = new Map<string, number>();
for (let i = 0; i < 1000; i = i + 1) {
  const kk = "w" + (i % 37);
  if (s.has(kk)) {
    s.set(kk, s.get(kk)! + i);
    hits = hits + 1;
  } else {
    s.set(kk, i);
  }
  if (i % 250 === 0) s.delete("w" + (i % 37));
}
let sum = 0;
for (const v of s.values()) sum = sum + v;
console.log(s.size, hits, sum);
