// 422-01 — `let v;` (no init, no annotation) inside a generator body
// lifts as an `any` field: the binding starts undefined and takes
// whatever a later assignment sends. The historical "number" fallback
// could not even hold the initial undefined
// ("type mismatch assigning to v: field is Number, value is
// Undefined"). Both the plain-yield and yield* resumption shapes.
function* g() {
  let v;
  v = yield 1;
  console.log("plain", v);
}
const it = g();
it.next();
it.next(5);

async function* inner() {
  yield 1;
  return 77;
}
async function* outer() {
  let v;
  v = yield* inner();
  console.log("star", v);
}
async function main() {
  for await (const x of outer()) console.log(x);
}
main();
