// §27.4.3 — every DisposableStack / AsyncDisposableStack method
// requires a receiver carrying [[DisposableState]]; a primitive or
// a plain object is a TypeError. The check used to be accidental —
// the method's first write to `this` was a strict [[Set]] on a
// primitive, which threw — and vanished when sloppy assignment
// learned to discard that refusal. It is an explicit brand check now.
const move: any = DisposableStack.prototype.move;
const use: any = DisposableStack.prototype.use;
const dispose: any = DisposableStack.prototype.dispose;
for (const recv of [undefined, null, true, 1, "s", Symbol("q"), {}]) {
  let caught = 0;
  try { move.call(recv); } catch (e: any) { if (e instanceof TypeError) caught++; }
  try { use.call(recv, {}); } catch (e: any) { if (e instanceof TypeError) caught++; }
  try { dispose.call(recv); } catch (e: any) { if (e instanceof TypeError) caught++; }
  console.log(typeof recv, caught);
}
const amove: any = AsyncDisposableStack.prototype.move;
try { amove.call(5); console.log("no throw"); } catch (e: any) { console.log("async", e instanceof TypeError); }
const s = new DisposableStack();
const moved = s.move();
console.log(s.disposed, moved.disposed);
