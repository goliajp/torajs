// S248 — Set.add / Map.set trailing-arg ignore per ES §24.2.3.1 /
// §23.1.3.9. Useful args type-checked normally, trailing operands
// dropped at lower-time. Receiver returned (chained idiom).

// --- Set<number>.add(value, ...trailing) ---
const s1 = new Set<number>();
s1.add(1, 99);
console.log(s1.size, s1.has(1), s1.has(99));
s1.add(2, 100, 200);
console.log(s1.size, s1.has(2), s1.has(100));
s1.add(3, 1, 2, 3, 4, 5);
console.log(s1.size);

// --- Set<string>.add(value, ...trailing) ---
const s2 = new Set<string>();
s2.add("a", "junk1");
s2.add("b", "junk2", "junk3");
console.log(s2.size, s2.has("a"), s2.has("b"), s2.has("junk1"));

// --- Map<string, number>.set(key, value, ...trailing) ---
const m1 = new Map<string, number>();
m1.set("k", 42, 99);
console.log(m1.size, m1.get("k"));
m1.set("k2", 100, 1, 2, 3);
console.log(m1.size, m1.get("k2"));

// --- Map<number, string>.set ---
const m2 = new Map<number, string>();
m2.set(1, "one", "extra");
m2.set(2, "two", "extra1", "extra2");
console.log(m2.size, m2.get(1), m2.get(2));

// --- chained idiom Set ---
const s3 = new Set<number>();
s3.add(10, 1).add(20, 2).add(30, 3);
console.log(s3.size, s3.has(10), s3.has(20), s3.has(30));

// --- chained idiom Map ---
const m3 = new Map<string, number>();
m3.set("a", 1, 99).set("b", 2, 88).set("c", 3, 77);
console.log(m3.size, m3.get("a"), m3.get("b"), m3.get("c"));
