// `Promise.resolve(x)` on an `any` argument answers the SAME cell when
// x is already a promise (§27.2.4.7 step 2), so the result carries the
// source promise's storage form — not the boxed one a `Promise<any>`
// await assumes. Reading the slot raw and rc_inc'ing it as a NaN box
// is rc_inc(0x1).
async function main() {
  var p = Promise.resolve(1);
  const q = Promise.resolve(p);
  console.log("same cell", q === p);
  console.log("await i64", await q);

  var ps = Promise.resolve("s");
  console.log("await str", await Promise.resolve(ps));

  var pf = Promise.resolve(1.5);
  console.log("await f64", await Promise.resolve(pf));

  var pb = Promise.resolve(true);
  console.log("await bool", await Promise.resolve(pb));

  var po = Promise.resolve([1, 2]);
  const arr = await Promise.resolve(po);
  console.log("await heap", arr.join(","));

  // a non-promise `any` still mints a fresh fulfilled cell
  var raw = 7;
  console.log("await plain", await Promise.resolve(raw));

  // the typed argument keeps the typed read
  const t = Promise.resolve(1);
  console.log("typed", await Promise.resolve(t));
}
main();
