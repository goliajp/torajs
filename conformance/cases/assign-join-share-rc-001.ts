// Rotation 326 — the assignment face of the borrow-join defect the
// let-decl shares table took earlier this rotation: `y = cond ? ys :
// xs` joins two borrows (chunk 722 keeps such joins at zero rc
// traffic), and the assign lane's borrow whitelist had no join arm —
// the store took the borrow as if it owned it, and y's scope-end
// drop stole xs's stake.
function assignTernary(): void {
  const xs = [10, 20, 30];
  const ys = [1, 2, 3];
  let y = ys;
  y = false ? ys : xs;
  y.shift();
  console.log(xs[0], xs.length, ys.length);
}
assignTernary();

function assignLogical(): void {
  const a = [1, 2];
  const b = [3, 4, 5];
  let z = a;
  z = a && b;
  console.log(z.length, a.length, b.length);
}
assignLogical();
console.log("done");
