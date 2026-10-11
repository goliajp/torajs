// A generator reading a top-level name that a scope between it and the
// top level rebinds reads that binding.
const k = 1;
function exprParam(k: number) { const it = (function* () { yield k })(); return it.next().value }
function declParam(k: number) { function* gen() { yield k } return gen().next().value }
function kf() { return 1 }
function declFnParam(kf: () => number) { function* gen() { yield kf() } return gen().next().value }
function exprLocal(x: number) { let k = x + 1; const it = (function* () { yield k })(); return it.next().value }
const arrowDecl = (k: number) => { function* gen() { yield k } return gen().next().value };
function forOf(x: number) {
  let r = 0;
  for (const k of [x + 2]) { function* gen() { yield k } r = gen().next().value as number }
  return r;
}
console.log(exprParam(6), declParam(6), declFnParam(() => 2), exprLocal(6), arrowDecl(6), forOf(6), k, kf());
