// S250 — Date.<static>(...trailing) trailing-arg ignore per ES
// §21.4.3.*: Date.now / Date.parse / Date.UTC each read only their
// declared positional args; trailing operands type_of'd then dropped
// at SSA-emit time. Same narrow shape as S243 Math.* (Type::Object
// receiver carve-out).

// --- Date.now() ignore trailing ---
const t1 = Date.now(99);
console.log(typeof t1);
const t2 = Date.now(1, 2, 3);
console.log(typeof t2);
const t3 = Date.now("junk", true);
console.log(typeof t3);

// --- Date.parse(iso, ...trailing) ---
const p1 = Date.parse("2020-01-01T00:00:00Z", 99);
console.log(p1);
const p2 = Date.parse("2020-01-01T00:00:00Z", 1, 2, 3);
console.log(p2);

// --- Date.UTC(y, m, d, ..., trailing) ---
const u1 = Date.UTC(2020, 0, 1, 0, 0, 0, 0, 99);
console.log(u1);
