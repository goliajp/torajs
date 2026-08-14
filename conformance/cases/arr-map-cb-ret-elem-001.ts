// 403-01 — a map result element is the callback return, wherever the
// call sits (the sniff no longer pins an enclosing fn to same-T).
function runA() {
  return [1].map(function (x: number) { return typeof this; })[0];
}
console.log(runA());
function runB() {
  const r = [1, 2].map(function (x: number) { return "s" + x; });
  console.log(r);
  return r[1];
}
console.log(runB());
function runC() {
  return [3].map((x: number) => x + 1)[0];
}
console.log(runC());
class K {
  m() {
    return [1].map(function (x: number) { return typeof this; })[0];
  }
}
console.log(new K().m());
console.log([9].map(function (x: number) { return "t" + x; })[0]);
