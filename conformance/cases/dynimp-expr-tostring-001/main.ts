// §13.3.10 — dynamic import with a non-literal specifier: ToString
// runs the object's toString, and the resulting path resolves against
// the baked candidate table (the literal appears in this source).
//
// The two imports are sequenced by the program on purpose. Left to
// race, which resolves first is the host loader's business — bun
// answers both orders across runs, and the gate's oracle cache froze
// one of them, so the fixture read as stable while it was not.
const obj: any = {
  toString() {
    return "./mod.ts";
  },
};
async function main(): Promise<void> {
  const viaToString: any = await import(obj);
  console.log("tostring", viaToString.local1, viaToString.greet());
  const viaLiteral = await import("./mod.ts");
  console.log("lit", viaLiteral.local1);
}
main();
