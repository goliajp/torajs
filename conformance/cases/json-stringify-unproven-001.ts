// JSON.stringify of a pointer binding the live-field census does not
// prove live keeps the static unfold: the undefined cell answers
// undefined, NULL answers "null", and a live value serializes as its
// static type does, toJSON hooks receiving their property keys
function count() {
  return arguments.length;
}

var top = { p: { toJSON: function (key: any) { return "hook:" + key; } }, n: 1 };
console.log(JSON.stringify(top), count(1, 2));

type Box = { v: number; tag: string };
let held: Box | null = { v: 1, tag: "a" };
function clear() {
  held = null;
}
function show(b: Box) {
  return JSON.stringify(b);
}
if (held !== null) {
  console.log(show(held));
  clear();
  const b: Box = held;
  console.log(show(b), JSON.stringify(b));
}

const maybe: Box | undefined = Math.random() > 2 ? { v: 2, tag: "b" } : undefined;
console.log(JSON.stringify(maybe), typeof JSON.stringify(maybe));

const boxes: Box[] = [{ v: 3, tag: "c" }];
boxes.length = 2;
for (const b of boxes) console.log(JSON.stringify(b));

var when = new Date(0);
var dict = new Map<string, number>([["k", 1]]);
var list = [1, 2, { toJSON: (k: any) => "at:" + k }];
console.log(JSON.stringify(when), JSON.stringify(dict), JSON.stringify(list));
console.log(JSON.stringify(top, null, 2));

// an awaited value could hold the cell before the census judged reads,
// so it keeps the run-time walk
async function settled() {
  const ns = await Promise.allSettled([Promise.resolve(1), Promise.reject(2)]);
  console.log(JSON.stringify(ns));
}
settled();
