// bug-327 fourth value shape — a named top-level fn stored in an
// object-literal field is Closure repr; reading it back and calling
// through an untyped (implicit-generic) or fn-typed parameter must
// dispatch env-first, not blr the cell's heap header.
function check(fn) {
  console.log("got", typeof fn, fn());
}
function typedCheck(fn: () => number): number {
  return fn();
}
function h(): number {
  return 42;
}
let o = { f: h };
check(o.f);
console.log("typed", typedCheck(o.f));
const c = o.f;
check(c);
