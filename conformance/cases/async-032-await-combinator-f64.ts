// r3/r4 — default-Any async fn awaiting concrete-typed promises:
// the declared Promise<any> return must absorb the body's inferred
// Promise<number> (assignability covariance), and the awaited value
// slot must decode f64 bits through array/combinator projections.

// r4 — Index projection: await ps[i] on Array<Promise<number>>
async function awaitIndexed() {
  let ps = [Promise.resolve(2.5)];
  let v = await ps[0];
  console.log(v);
}

// r3 — Promise.all fan-in: result array's element face is f64
async function awaitAll() {
  let r = await Promise.all([Promise.resolve(1.5), Promise.resolve(2.5)]);
  console.log(r[0] + r[1]);
  console.log(r[1]);
}

// race / any share the single-value fan-in wiring
async function awaitRace() {
  let v = await Promise.race([Promise.resolve(3.5)]);
  console.log(v);
}

async function awaitAny() {
  let v = await Promise.any([Promise.resolve(4.5)]);
  console.log(v);
}

// integer faces must stay narrow through the same paths
async function awaitAllInt() {
  let r = await Promise.all([Promise.resolve(2), Promise.resolve(3)]);
  console.log(r[0] + r[1]);
}

async function main() {
  await awaitIndexed();
  await awaitAll();
  await awaitRace();
  await awaitAny();
  await awaitAllInt();
}
main();
