// A for-in loop whose body suspends — a `yield` in a generator, an
// `await` in an async function. The state machine had no arm for it,
// so the program was refused ("yield is only valid inside a
// `function*` generator body"). It is now a walk over the key snapshot
// the loop takes at entry, skipping a key deleted before it is reached.
function* pairs(o: any) {
  for (const k in o) yield k + "=" + o[k];
}
console.log([...pairs({ a: 1, b: 2, c: 3 })].join(","));

function* deleting(o: any) {
  for (const k in o) {
    if (k === "a") delete o.b;
    yield k;
  }
}
console.log([...deleting({ a: 1, b: 2, c: 3 })].join(","));

function* adding(o: any) {
  for (const k in o) {
    if (k === "a") o.z = 9;
    yield k;
  }
}
console.log([...adding({ a: 1, b: 2 })].join(","));

function* indices(xs: number[]) {
  for (const i in xs) yield i + ":" + xs[i as any];
}
console.log([...indices([5, 6])].join(","));

function* labeled(o: any) {
  outer: for (const k in o) {
    for (const j in o) {
      if (j === k) continue outer;
      if (k === "c") break outer;
      yield k + j;
    }
  }
  yield "end";
}
console.log([...labeled({ a: 1, b: 2, c: 3 })].join(","));

function* varHead(o: any) {
  for (var k in o) yield k;
  yield "last:" + k;
}
console.log([...varHead({ x: 1, y: 2 })].join(","));

const box: any = {};
function* memberHead(o: any) {
  for (box.p in o) yield box.p;
}
console.log([...memberHead({ m: 1, n: 2 })].join(","), box.p);

function* early(o: any) {
  for (const k in o) {
    if (k === "b") return;
    yield k;
  }
}
console.log([...early({ a: 1, b: 2, c: 3 })].join(","));

async function collect(o: any): Promise<string> {
  const out: string[] = [];
  for (const k in o) {
    await null;
    out.push(k);
  }
  return out.join(",");
}
collect({ u: 1, w: 2 }).then((v) => console.log(v));
