// ES §13.1.1 — at a Script's top level `await` is an
// IdentifierReference wherever that reading parses, so each of these
// is a call, an index or an operator on a binding named `await`.
function await(x) {
  console.log("called", x);
  return [10, 20, 30];
}
const viaCall = await(undefined);
console.log(viaCall.length);
console.log(await(1)[2]);
const pending = [];
pending.push(await("in-array"));
console.log(pending.length);
async function real() {
  // inside an async body the word is the operator again
  const v = await Promise.resolve(5);
  return v + 1;
}
real().then((v) => console.log("async", v));
