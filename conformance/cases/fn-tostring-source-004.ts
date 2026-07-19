// RFC 20260719-fn-tostring-source B6a — static-tier toString on
// fn-typed VALUES (closure bindings / fn params) routes through the
// runtime erased-source kernels; generator + bare-arrow faces ride
// the recorded decl spans. (fn-EXPRESSION whitespace is a recorded
// divergence: bun's transpiler reflows `function (y) {...}`.)
const bare = (x: number) => x + 1;
console.log(bare.toString());
function take(cb: (n: number) => number): string {
  return cb.toString();
}
console.log(take(bare));
console.log(take((z: number) => z * 9));
function* gen(n: number): Generator<number> {
  yield n;
}
const g: any = gen;
console.log(g.toString());
console.log(bare(1));
