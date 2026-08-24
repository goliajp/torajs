// mid 171 double-booking regression (ANY_METHOD_TAKE vs
// ANY_METHOD_GET_OR_INSERT_COMPUTED): a Map's getOrInsertComputed
// must never be reachable as `take`, and an iterator helper must
// reject getOrInsertComputed — before the re-home, the shared id
// silently ran take(1) for the iterator call below.
const m = new Map();
const am = m as any;
try {
  am.take(2);
  console.log("map-take:no-throw", m.size);
} catch (e) {
  console.log("map-take:threw");
}
const it = [1, 2, 3].values() as any;
try {
  it.getOrInsertComputed(1, () => 9);
  console.log("iter-goic:no-throw");
} catch (e) {
  console.log("iter-goic:threw");
}
// the real methods keep working on their own families
const m2 = new Map<number, number>();
const got = (m2 as any).getOrInsertComputed(1, () => 42);
console.log("map-goic:", got, m2.size);
const taken = ([10, 20, 30].values() as any).take(2).toArray();
console.log("iter-take:", taken);
