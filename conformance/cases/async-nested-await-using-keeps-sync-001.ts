// A nested async function value holding `await using` keeps the
// synchronous lowering (blade 5 has not moved it), and so does the
// body enclosing it: the two lowerings do not yet compose (a labeled
// `break` past the declaration crashed the cycle collector at exit
// when the outer body was a machine). Pinned so the crash cannot
// come back unnoticed; once blade 5 moves `await using` bodies the
// case still holds.
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
