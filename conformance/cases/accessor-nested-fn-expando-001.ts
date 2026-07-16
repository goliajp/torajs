// RFC 20260717 closure-env-cycle residual ② — a NESTED named fn as an
// accessor face lowers to a zero-capture minted env on the trivial
// drop (the top-level forwarder pass doesn't cover nested fns), and
// expando writes on the reified getter cell must round-trip AND be
// released by the trivial drop (pre-fix: 700MB RSS on the 300k churn
// variant of this shape; post-fix flat).
function run(i: number): number {
  function getN(): number {
    return 7;
  }
  const o: any = {};
  Object.defineProperty(o, "x", { get: getN });
  const g: any = Object.getOwnPropertyDescriptor(o, "x").get;
  g.tag = "expando-" + i;
  const back: any = Object.getOwnPropertyDescriptor(o, "x").get;
  console.log(back.tag);
  return o.x;
}
let sum = 0;
for (let i = 0; i < 3; i++) {
  sum += run(i);
}
console.log(sum);
