// An element literal outside any declared slot keeps its fraction even
// when a declared type of the same shape exists: an un-annotated array,
// an `any[]` and an array of a union.
type A = { right: number };
const xs = [{ right: 3.5 }];
console.log(xs[0].right);

const ys: any[] = [{ right: 3.5 }, { right: 2 }];
console.log(ys[0].right, ys[1].right);

type Add = { tag: "add"; left: number; right: number };
type N = Add | { tag: "num"; value: number };
const ns: N[] = [
  { tag: "add", left: 2, right: 3.5 },
  { tag: "num", value: 0.75 },
];
for (const n of ns) {
  if (n.tag === "add") console.log("add", n.left, n.right, n.left + n.right);
  else console.log("num", n.value);
}

const declared: A = { right: 9 };
console.log(declared.right);
