// RFC 20260729-fn-value-any V1 — a named top-level function passed
// as a member-call argument boxes into the any world when the
// receiver is not a statically-typed ident: an `any`-bound receiver
// (the t262 async harness's `.then($DONE, $DONE)` shape) and a
// chained-expression receiver both wrap the fn through its
// forwarder closure; a typed ident receiver keeps raw-FnSig
// dispatch (regression leg).
function done(e: any = undefined): void {
  console.log("done", e);
}
function double(n: number): number {
  return n * 2;
}
const p: any = Promise.resolve(1);
p.then(done, done);

function mk(): any {
  return Promise.resolve(7);
}
mk().then(done);

const xs: number[] = [1, 2, 3];
console.log(xs.map(double));
