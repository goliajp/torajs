// Every function one call site may reach must agree on a single number
// width for its parameters and its result, however the callee is chosen.
// Each shape below uses its own pair so no other use joins them.
function apply(f: (n: number) => number, n: number) { return f(n) }
function ternSq(n: number) { return n * n }
function ternDbl(n: number) { return n + n }
function orSq(n: number) { return n * n }
function orHalf(n: number) { return n / 2 }
function nulSq(n: number) { return n * n }
function nulHalf(n: number) { return n / 2 }
function letSq(n: number) { return n * n }
function letHalf(n: number) { return n / 2 }
function arrSq(n: number) { return n * n }
function arrHalf(n: number) { return n / 2 }
const fsArr = [arrSq, arrHalf];
function litSq(n: number) { return n * n }
function litHalf(n: number) { return n / 2 }
function objSq(n: number) { return n * n }
function objHalf(n: number) { return n / 2 }
const oObj = { a: objSq, b: objHalf };
function argSq(n: number) { return n * n }
function argHalf(n: number) { return n / 2 }
function facSq(n: number) { return n * n }
function facHalf(n: number) { return n / 2 }
function pick(c: boolean) { return c ? facSq : facHalf }
function pInc(n: number) { return n + 1 }
function pDbl(n: number) { return n + n }
function rInc(n: number): number { return n + 1 }
function rHalf(n: number): number { return n / 2 }
function oSq(n: number) { return n * n }
function oDbl(n: number) { return n + n }
function qSq(n: number) { return n * n }
function qDbl(n: number) { return n + n }
const ca = (n: number) => n + 1;
const cb = (n: number) => n / 2;
const da = (n: number) => n + 1;
const db = (n: number) => n + n;
function iA(n: number) { return n + 1 }
function iB(n: number) { return n * 3 }
function jA(n: number) { return n + 1 }
function jB(n: number) { return n * 3 }
function wrap(c: boolean, k: number) { return (c ? jA : jB)(k) }
for (let k = 0; k < 4; k++) {
  const c = k % 2 === 0;
  console.log("tern", (c ? ternSq : ternDbl)(k));
  console.log("or", ((c && orSq) || orHalf)(k));
  console.log("nullish", ((c ? nulSq : null) ?? nulHalf)(k));
  let f = c ? letSq : letHalf;
  console.log("let", f(k));
  console.log("arr", fsArr[k % 2](k));
  console.log("arrlit", [litSq, litHalf][k % 2](k));
  console.log("obj", (c ? oObj.a : oObj.b)(k));
  console.log("arg", apply(c ? argSq : argHalf, k));
  console.log("factory", pick(c)(k));
  console.log("param", (c ? pInc : pDbl)(k + 0.5));
  console.log("annotated", (c ? rInc : rHalf)(k + 1));
  console.log("or-int", ((c && oSq) || oDbl)(k));
  console.log("nullish-int", ((c ? qSq : null) ?? qDbl)(k));
  console.log("closure-local", (c ? ca : cb)(k));
  console.log("closure-param", (c ? da : db)(k + 0.25));
  console.log("inline", (c ? (n: number) => n + 1 : (n: number) => n / 2)(k));
  console.log("arg-param", apply(c ? iA : iB, k + 0.5));
  console.log("nested-fn", wrap(c, k));
}
