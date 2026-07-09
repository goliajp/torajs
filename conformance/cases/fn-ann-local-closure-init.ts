// chunk 734 — an immutable fn-typed local whose init is a capturing
// or plain arrow re-reprs its slot Closure (pre-fix: the FnSig slot
// dispatched the env pointer as a code address, SIGBUS in any
// non-toplevel scope). Named-fn inits keep direct dispatch via the
// fn_addr_let lane.
function viaParam(x: number): number {
  const f: () => number = () => x;
  return f();
}
function plain(): number {
  const f: () => number = () => 42;
  return f();
}
function named(n: number): number {
  return n * 2;
}
function viaNamed(): number {
  const f: (n: number) => number = named;
  return f(21);
}
console.log(viaParam(7));
console.log(plain());
console.log(viaNamed());
let total = 0;
for (let round = 0; round < 3; round++) {
  const f: () => number = () => round;
  total += f();
}
console.log(total);
const base = 9;
{
  const g: () => number = () => base;
  console.log(g());
}
const strf: (s: string) => string = (s: string) => s + "!";
console.log(strf("ok"));
