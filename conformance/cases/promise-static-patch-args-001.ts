// A patched `Promise.resolve` FN-EXPRESSION whose body reads
// `arguments` — the member-store into the Promise static slot joins
// the boxed-face store profile (both read-back channels enter the
// closure cell's boxed dual entry), so the body rides the argv face:
// real argc/argv on the direct any-lane call AND on the combinators'
// per-element consult (§27.2.4.1.3 step 6.i).
Promise.resolve = function () {
  console.log("p", arguments.length, typeof arguments[0]);
  return arguments[0];
};
function mk(v: any): any {
  return new Promise(function (res: any) { res(v); });
}
async function main() {
  const direct: any = await (Promise as any).resolve(7, 8);
  console.log("direct", direct);
  const vals: any = await Promise.all([mk(1), mk(2)]);
  console.log("all", vals[0], vals[1]);
}
main();
