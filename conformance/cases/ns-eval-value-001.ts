// §19.2.1 — the global `eval` as a first-class VALUE: thisArg
// identity, typeof, detached-binding identity and the name / length
// reflection. The call-through-a-value face is the recorded loud
// TypeError (tr performs no runtime evaluation), so this fixture
// stays on the surfaces bun and tr agree on; direct literal calls
// keep compiling through the desugar_eval prefix.
function cb(this: any) {
  return this === eval;
}
console.log([11].every(cb, eval));
console.log(typeof eval);
var f: any = eval;
console.log(f === eval);
console.log(f.length, f.name);
console.log(eval.length);
console.log(eval("1 + 1"));
