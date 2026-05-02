// Latent UAF: throwing a refcounted value across a fn boundary
// previously freed the value in the throwing fn's emit_drops walk
// before the caller's catch could read it. Stmt::Throw now mirrors
// Stmt::Return's consume-walk so the source local isn't double-
// dropped — the throw_value global takes ownership for the catch
// to pick up.
type Item = { name: string, count: number };
type Box = { items: Item[], total: number };

function makerItem(): void {
  let it: Item = { name: "made", count: 100 };
  throw it;
}

function makerBox(): void {
  let bx: Box = {
    items: [{ name: "alpha", count: 1 }, { name: "beta", count: 2 }],
    total: 3,
  };
  throw bx;
}

function check(): number {
  // Cross-fn refcounted throw: maker's local would otherwise be freed
  // by emit_drops on the way out, leaving e dangling.
  try {
    makerItem();
  } catch (e: Item) {
    if (e.name !== "made") { throw "#1: name"; }
    if (e.count !== 100) { throw "#2: count"; }
  }

  // Box with nested array — multi-layer refcount through the throw.
  try {
    makerBox();
  } catch (e: Box) {
    if (e.items.length !== 2) { throw "#3: len"; }
    if (e.items[0].name !== "alpha") { throw "#4"; }
    if (e.items[1].count !== 2) { throw "#5"; }
    if (e.total !== 3) { throw "#6"; }
  }

  // Re-throw still works (catch + re-raise).
  let saved: number = 0;
  try {
    try {
      makerItem();
    } catch (e: Item) {
      saved = e.count;
      throw e;
    }
  } catch (e2: Item) {
    if (e2.name !== "made") { throw "#7: re-throw"; }
    if (saved !== 100) { throw "#8"; }
  }
  return 0;
}
console.log(check());
