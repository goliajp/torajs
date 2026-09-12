// A captured `let` shadowed by a same-named captured `let` in an inner
// scope. The boxed-binding mark was keyed by NAME, so the inner plain
// binding inherited the outer's mark: its capture write took the byref
// path on a bare alloca and the program died with EXC_ARM_DA_ALIGN
// before its first line (test262 for-in / for-of `scope-body-lex-open`).
// The mark now follows the binding (`LocalInfo::boxed`).
var probeBefore = function () { return x; };
let x = "outside";
var probeBody: any = null;
{
  let x = "block";
  probeBody = function () { return x; };
}
console.log(probeBefore(), probeBody());
let y = "outer-y";
const grab = () => y;
for (let [y] of [["head"]]) {
  probeBody = function () { return y; };
}
console.log(grab(), probeBody());
function inner() {
  var before = function () { return z; };
  let z = "fn-outer";
  let after: any = null;
  { let z = "fn-block"; after = function () { return z; }; }
  return before() + "/" + after();
}
console.log(inner());
