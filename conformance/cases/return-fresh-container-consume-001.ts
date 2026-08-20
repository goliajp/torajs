// rotation 460 — the return consume walk marks every non-Copy local
// the return expression touches as moved, so the scope-exit drop
// skips it. That is right only when the returned value ALIASES the
// local's heap. An object literal, an array literal and a non-logical
// BinOp all answer a FRESH allocation whose stores take their own +1
// off a borrowed read, so the moved mark stole a drop nobody
// replaced: each of the three leaked one string per call (13.2MB vs
// 6.7MB RSS over 200k). The short-circuiting `&&` / `||` DO hand back
// an operand and keep the walk.
function viaObject(n: number): any {
  let cap = "a" + n;
  return { v: cap };
}
function viaArray(n: number): any {
  let cap = "a" + n;
  return [cap];
}
function viaConcat(n: number): any {
  let cap = "a" + n;
  return cap + "!";
}
function viaOr(n: number): any {
  let cap = "a" + n;
  return cap || "fallback";
}
function viaTernary(n: number): any {
  let cap = "a" + n;
  return n > 0 ? cap : "neg";
}
console.log(viaObject(1).v);
console.log(viaArray(2)[0]);
console.log(viaConcat(3));
console.log(viaOr(4));
console.log(viaTernary(5));
