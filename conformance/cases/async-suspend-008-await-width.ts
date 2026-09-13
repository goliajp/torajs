// `let xs: number[] = await e` inside a moved async body kept the
// synchronous lowering until now: after the move the binding is
// `let xs = this.__sent` — the value the driver sent back — and the
// width analysis saw no link between it and the literal `e` resolved
// with, so a fractional write into an int-born array was refused
// (`arr-promise-elem-width-001` documents the un-moved shape). The
// expansion now records the sent-slot read's await origin and the
// analysis reads it as `e.value`, the promise's value point, so the
// binding and the literal share one width class and the function
// moves like any other: every shape here suspends at its await.
async function decl(): Promise<void> {
  const p = Promise.resolve([1, 2, 3]);
  const xs: number[] = await p;
  xs[0] = 1.5;
  console.log("decl", xs[0], xs[1], xs[2]);
  const ys: number[] = await Promise.resolve([4, 5, 6]);
  ys[1] = 0.5;
  console.log("decl-direct", ys[0], ys[1], ys[2]);
}
const value = async (): Promise<void> => {
  const zs: number[] = await Promise.resolve([7, 8]);
  zs[0] = 7.25;
  console.log("value", zs[0], zs[1]);
};
class K {
  base = 10;
  async m(): Promise<number> {
    const ws: number[] = await Promise.resolve([1, 2]);
    ws[1] = 2.5;
    return this.base + ws[0] + ws[1];
  }
}
(async () => {
  await decl();
  await value();
  console.log("method", await new K().m());
})();
