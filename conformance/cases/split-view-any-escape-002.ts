// More ways an element of a split product leaves the split array
// through the any lane: `at`, a string-keyed index read, slice and
// splice through an `any` receiver, Object.values / entries /
// fromEntries / assign / groupBy, and a property descriptor. Each used
// to hand out the substring view itself, whose storage belongs to the
// split array, so the value printed another string's bytes once the
// array was gone.
function churn(): void {
  const j: string[] = [];
  for (let i = 0; i < 50; i++) j.push("vvvvvvvvv" + i);
}
function cells(n: number): any {
  return ("a" + n + ",b" + n + ",c" + n).split(",");
}

function at(n: number): any {
  return cells(n).at(1);
}
function keyed(n: number): any {
  const c = cells(n);
  let y: any = 0;
  for (const k in c) y = c[k];
  return y;
}
function slice(n: number): any {
  return cells(n).slice(1)[0];
}
function splice(n: number): any {
  return cells(n).splice(0, 2)[1];
}
function values(n: number): any {
  return Object.values(cells(n))[2];
}
function entries(n: number): any {
  return Object.entries(cells(n))[0][1];
}
function fromEntries(n: number): any {
  return Object.fromEntries([("k=" + n).split("=")]).k;
}
function assign(n: number): any {
  return Object.assign({}, cells(n))[1];
}
function groupBy(n: number): any {
  return Object.groupBy(cells(n), (s: any) => s[0]).b[0];
}
function mapGroupBy(n: number): any {
  return Map.groupBy(cells(n), (s: any) => s[0]).get("c")[0];
}
function descriptor(n: number): any {
  return Object.getOwnPropertyDescriptor(cells(n), 0)!.value;
}

const got = [
  at(1),
  keyed(2),
  slice(3),
  splice(4),
  values(5),
  entries(6),
  fromEntries(7),
  assign(8),
  groupBy(9),
  mapGroupBy(10),
  descriptor(11),
];
churn();
console.log(got.join(" "));
