// A callback handed an array's object elements reads them through the
// element's own widened layout: map, filter / find, forEach, sort,
// reduce, a named callback and Array.from's mapper.
type A = { right: number };
const xs: A[] = [{ right: 1 }, { right: -0.5 }];
console.log(xs.map((m) => m.right).join(","));
console.log(xs.filter((m) => m.right < 0).length, xs.find((m) => m.right < 0)!.right);
xs.forEach((m) => console.log(m.right));
const sorted = [...xs].sort((p, q) => p.right - q.right);
console.log(sorted[0].right);
console.log(xs.reduce((s, m) => s + m.right, 0));
function getR(m: A): number {
  return m.right;
}
console.log(xs.map(getR).join(","));
console.log(Array.from(xs, (m) => m.right * 2).join(","));
