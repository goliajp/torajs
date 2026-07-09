// chunk 736 — a MUTABLE fn-typed binding initialized with a bare
// named fn wraps through __forward_<name> so the slot lives as a
// closure cell from birth (pre-fix: fn_addr_let pinned a FnSig slot
// and a later arrow reassign hit the slot-mismatch loud panic).
// Covers fn-body and toplevel homes, named->named and named->arrow
// reassigns; immutable named-fn inits keep direct dispatch.
function inc(n: number): number {
  return n + 1;
}
function dec(n: number): number {
  return n - 1;
}
function body(): void {
  let cb: (n: number) => number = inc;
  console.log(cb(5));
  cb = dec;
  console.log(cb(5));
  cb = (n: number) => n * 10;
  console.log(cb(5));
}
body();
let top: (n: number) => number = inc;
console.log(top(7));
top = dec;
console.log(top(7));
top = (n: number) => n * 100;
console.log(top(7));
const direct: (n: number) => number = inc;
console.log(direct(9));
