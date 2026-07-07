// RFC 20260705 Phase 2 close-out — sibling closures capturing the
// same typed array stay coherent across push-driven grows: chunk 582
// (B1) pinned the array cell and moved slots behind the data-ptr
// indirection, so a grow swaps the buffer and every env alias reads
// the fresh slots (pre-B1 the native realloc freed the old block and
// the second env held a stale pointer — the capture-box indirection
// this RFC deferred is structurally unnecessary now).
function run(): void {
  const xs: number[] = [1];
  const pusher = () => {
    for (let i = 0; i < 5000; i++) xs.push(i);
  };
  const reader = () => xs.length;
  pusher();
  console.log(reader());
  console.log(xs[4000]);
  const xs2: string[] = ["a"];
  const p2 = () => {
    for (let i = 0; i < 3000; i++) xs2.push("s" + i);
  };
  const r2 = () => xs2[xs2.length - 1];
  p2();
  console.log(r2());
}
run();
console.log("done");
