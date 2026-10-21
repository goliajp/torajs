// A `.then` leg that is not callable passes the settlement through
// (§27.2.5.4 PerformPromiseThen steps 3-4), and a top-level function
// passed as a leg settles its declared return; the handler after either
// takes that value.

function errFn(e: any): number {
  return -1;
}
function okFn(x: number): number {
  return x * 10;
}
function okStr(x: number): string {
  return "n" + x;
}

Promise.resolve(3).then((x) => x + 1, undefined).then((v) => console.log("undefined onErr", v + 1, typeof v));
Promise.resolve(3).then((x) => x + 1, null).then((v) => console.log("null onErr", v + 1, typeof v));
Promise.resolve(3).then(okFn, errFn).then((v) => console.log("named", v + 1, typeof v));
Promise.reject(new Error("r")).then(okFn, errFn).then((v) => console.log("named rejected", v + 1, typeof v));
Promise.resolve(4).then(undefined, (e) => 0).then((v) => console.log("undefined onOk", v + 1, typeof v));
Promise.resolve(4).then(null, (e) => 0).then((v) => console.log("null onOk", v + 1, typeof v));
Promise.resolve(7).catch(undefined).then((v) => console.log("catch undefined", v + 1, typeof v));
Promise.resolve(2).then(okStr, errFn).then((v) => console.log("named mixed", v, typeof v));
Promise.reject(new Error("q")).then(okStr, errFn).then((v) => console.log("named mixed rejected", v, typeof v));
Promise.resolve(5).then(okFn).then((v) => console.log("named one leg", v + 1));

async function body(): Promise<void> {
  const a = await Promise.resolve(6).then(undefined, (e) => 0);
  console.log("await undefined onOk", a + 1);
  const b = await Promise.resolve(6).then(okFn, errFn);
  console.log("await named", b + 1);
  const c = await Promise.reject(new Error("c")).then(okFn, errFn);
  console.log("await named rejected", c + 1);
}
body();
