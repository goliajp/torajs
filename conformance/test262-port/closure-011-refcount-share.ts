// Closure / fnsig calls previously consumed (marked moved) every
// non-Copy ident arg. For refcounted heap types this was a latent
// UAF: `let r = f(x); use(x)` would have caller skip x's drop while
// r dropped it (rc=0 free), then use(x) would touch freed memory.
// Now refcounted args rc_inc at the call site so caller stays the
// owner and callee gets its own ref.
type Item = { name: string, count: number };

function identityItem(it: Item): Item { return it; }
function consumeItem(it: Item): void {
  // Reads a field; doesn't store it externally.
  let _ = it.count;
}

function check(): number {
  let it1: Item = { name: "apple", count: 5 };

  // Closure-typed local. tr's old path consumed it1 here so the
  // caller's drop was skipped — but it1 still needed to be valid for
  // the next read.
  let f1: (it: Item) => Item = identityItem;
  let r1: Item = f1(it1);
  let r2: Item = f1(it1);
  let r3: Item = f1(it1);
  if (it1.name !== "apple") { throw "#1: it1 corrupted"; }
  if (r1.name !== "apple") { throw "#2"; }
  if (r2.count !== 5) { throw "#3"; }
  if (r3.name !== "apple") { throw "#4"; }

  // Mix of consume-style and reuse — caller still owns after each.
  let f2: (it: Item) => void = consumeItem;
  f2(it1);
  f2(it1);
  if (it1.name !== "apple") { throw "#5"; }
  if (it1.count !== 5) { throw "#6"; }

  // Pass to fnsig via let-bound global fn — same code path.
  let g: (it: Item) => Item = identityItem;
  let g1: Item = g(it1);
  let g2: Item = g(it1);
  if (g1.count !== g2.count) { throw "#7"; }
  if (it1.count !== 5) { throw "#8"; }
  return 0;
}
console.log(check());
