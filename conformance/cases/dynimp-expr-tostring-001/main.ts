// §13.3.10 — dynamic import with a non-literal specifier: ToString
// runs the object's toString, and the resulting path resolves against
// the baked candidate table (the literal appears in this source).
const obj: any = {
  toString() {
    return "./mod.ts";
  },
};
import(obj).then((ns: any) => {
  console.log("tostring", ns.local1, ns.greet());
});
async function viaAwait(): Promise<void> {
  const ns = await import("./mod.ts");
  console.log("lit", ns.local1);
}
viaAwait();
