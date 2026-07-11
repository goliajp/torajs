// RFC 20260711-closure-reflection chunk D — dynamic-key member
// access on closure receivers. `f[k]` routes through the
// borrow-shaped member probe pair; k == "name"/"length" answers the
// same fn metadata the static reads do (immortal interned name
// cells), tombstone-gated; writes refuse like the static path;
// expando keys round-trip. propertyHelper's verify* family reads
// exclusively through computed `obj[name]`, so this is the last leg
// of the name.js/length.js cluster.
//
// Acceptance: byte-equal with bun.

function named(a: number, b: number) { return a + b; }
const f: any = named;
const k1: string = "name";
const k2: string = "length";

// 1. dynamic reads answer the virtual pair
console.log(f[k1], f[k2]);

// 2. reified proto method cell through dynamic keys
const sp: any = String.prototype.slice;
console.log(sp[k1], sp[k2]);

// 3. dynamic write refuses (readonly), value holds
try { f[k2] = "unlikelyValue"; } catch (e) { console.log("dyn-write-threw", e instanceof TypeError); }
console.log("after", f[k2]);

// 4. dynamic delete (computed, concat-built key) tombstones
const k3: string = "len" + "gth";
console.log("dyn-del", delete f[k3], f.hasOwnProperty("length"));

// 5. expando via dynamic key round-trips
const k4: string = "custom";
f[k4] = 9;
console.log(f[k4]);

// 6. NamedEvaluation arrow through dynamic keys
const g: any = (x: number) => x;
console.log(g[k1], g[k2]);
