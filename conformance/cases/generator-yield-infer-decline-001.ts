// generators whose yields do not all settle one number or string type
// keep the any lane and answer exactly what they yield.
function* mixed() {
  yield 1;
  yield "two";
}
function* bare() {
  yield 1;
  yield;
}
function* delegate() {
  yield 0;
  yield* [1, 2];
}
function* withReturn() {
  yield 1;
  return 9;
}
function* fromAny(x: any) {
  yield x;
}
function* flags() {
  yield true;
  yield false;
}
function* objs() {
  yield { k: 1 };
}
async function* agen() {
  yield 5;
}

console.log([...mixed()], [...bare()], [...delegate()]);
const w = withReturn();
console.log(w.next().value, w.next().value, w.next().value);
console.log([...fromAny("s")], [...fromAny(3)], [...flags()], JSON.stringify([...objs()]));
(async () => {
  for await (const v of agen()) console.log("async", v);
})();
