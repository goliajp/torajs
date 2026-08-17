// 423-01 knife 4 — the test262 dynamic-import double-FIXTURE shape:
// two candidates whose bare-export originals (local2 → renamed),
// self re-export faces (indirect), defaults, and var exports
// (local1) all collide pairwise. The walk-time census now mangles a
// bare-exported decl on its FACE surface, the aliased nested fetch
// follows the path's mangle memory, and the promote gate admits the
// minted bindings — so both namespaces answer their own module's
// values.
async function fn(): Promise<void> {
  const ns1: any = await import("./lib_one.ts");
  console.log(ns1.local1, ns1.renamed, ns1.indirect, ns1.default);
  const ns2: any = await import("./lib_two.ts");
  console.log(ns2.local1, ns2.renamed, ns2.indirect, ns2.default);
}
fn();
