// proposal-array-from-async §2.1.1 — Array.fromAsync(items),
// sync-source MVP. The dyn kernel drives the Array.from step
// protocol (iterable walks GetIterator; anything else takes the
// §23.1.2.1 step-3 array-like branch), then unwraps settled promise
// elements per step 5.e — a rejected element rejects the result,
// null/undefined reject with the length-read TypeError, and a plain
// number answers an empty array. mapFn stays a loud compile reject
// (element semantics — silent trailing-drop would be wrong).

async function main(): Promise<void> {
  // sync iterable with promise + plain elements — step 5.e unwrap.
  const xs: any = [Promise.resolve(1), 2, "s"];
  const r1 = await Array.fromAsync(xs);
  console.log(r1.length, r1[0], r1[1], r1[2]);

  // string — spec-iterable, one element per character.
  const r2 = await Array.fromAsync("ab");
  console.log(r2.length, r2[0], r2[1]);

  // Set — default iterator is values().
  const s = new Set<number>();
  s.add(7);
  s.add(9);
  const r3 = await Array.fromAsync(s);
  console.log(r3[0], r3[1]);

  // array-like branch: a plain number has no length — empty array.
  const r4 = await Array.fromAsync(5);
  console.log(r4.length);

  // rejected element rejects the whole result.
  const bad: any = [Promise.reject("bad")];
  try {
    await Array.fromAsync(bad);
    console.log("BAD: fulfilled");
  } catch (e) {
    console.log("caught", e);
  }

  // null — the array-like length read throws.
  const n: any = null;
  try {
    await Array.fromAsync(n);
    console.log("BAD: null fulfilled");
  } catch (e) {
    console.log("null:", e instanceof TypeError);
  }
  console.log("done");
}
main();
