// §27.2.4.1.3 — a typed VALUE-kind array reaching the combinators
// through the as-cast road (the checker rejects the bare spelling)
// carries raw scalar slots, not promise pointers: each element is
// legal plain input that promiseResolve wraps. The lowering routes
// this shape through the dynamic entries (boxing stamps the element
// kind); the typed sync walk would read every slot as a cell.
async function main() {
  const a: any = await Promise.all([1, 2] as any);
  console.log("all", a[0], a[1]);
  const r: any = await Promise.race([10, 20] as any);
  console.log("race", r);
  // the record's fields ride the dyn entry's anonymous-record
  // posture (record_tags = 0, recorded residue) — pin the length.
  const s: any = await Promise.allSettled([true] as any);
  console.log("settled", s.length);
  const y: any = await Promise.any([1.5] as any);
  console.log("any", y);
}
main();
