// §14.7.5.9: a property deleted before for-in reaches it is not
// visited. The delete here happens on `this` inside a getter the loop
// body reads, not on the loop's own binding.
const r = {
  get a() {
    delete this.b;
    return 1;
  },
  b: 2,
  c: 3,
};
const seen: string[] = [];
for (const k in r) {
  seen.push(k + "=" + String((r as any)[k]));
}
console.log(seen.join());

class Box {
  first = 1;
  second = 2;
  third = 3;
}
const box = new Box();
const order: string[] = [];
for (const k in box) {
  order.push(k);
  if (k === "first") Reflect.deleteProperty(box, "second");
}
console.log(order.join());

// The same check inside a generator body, whose for-in lowers through
// its own resumable loop.
class Row {
  x = 1;
  y = 2;
  z = 3;
}
function* walk(o: Row) {
  for (const k in o) {
    if (k === "x") Reflect.deleteProperty(o, "y");
    yield k;
  }
}
console.log([...walk(new Row())].join());

// A delete of a name the layout does not declare leaves every
// declared key visited.
const p = { u: 1, v: 2 };
const q: any = { w: 3 };
const pk: string[] = [];
for (const k in p) {
  delete q.w;
  pk.push(k);
}
console.log(pk.join(), Object.keys(q).length);
