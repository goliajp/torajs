// desugar_async used to scan only top-level stmts: an async fn
// declared inside another fn's body kept its bare returns — annotated
// bodies failed check (`Promise(Number)` vs `Number`), unannotated
// ones leaked the bare value out of the Promise lane (probe-aw6).
// `typeof` on the un-awaited call result is the discriminator: a
// leaked bare 42 answers "number", a real Promise answers "object".
async function outer(): Promise<number> {
  async function inner(): Promise<number> {
    return 41 + 1;
  }
  const p = inner();
  console.log(typeof p);
  const v = await p;
  return v * 2;
}
async function main() {
  const r = await outer();
  console.log(r);
  async function innerNoAnn() {
    return 7;
  }
  const q = innerNoAnn();
  console.log(typeof q);
  console.log(await q);
}
main();
