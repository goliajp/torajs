// A nested async function value holding `await using` inside an
// async function that is itself a machine: both move (RFC 20260915
// blade 5), and the two machines compose — the labeled `break` past
// the declaration resolves to the block's exit state (it once
// crashed the cycle collector at exit when the outer body was a
// machine and the inner one kept the synchronous lowering). The
// "same" flag flips after the inner call returns at its first
// suspend, which is what the two `true`s observe.
async function outer(): Promise<any> {
  var same = true; var seenBody = false; var seenAfter = false;
  async function f() {
    lbl: { seenBody = same; if (same) break lbl; await using _ = null; }
    seenAfter = same;
  }
  var p = f(); same = false; await p;
  return seenBody + " " + seenAfter;
}
outer().then((v: any) => console.log("nested-await-using", v));
