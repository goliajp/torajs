// Mixed-inner-type nested array literals must ride the Arr<Any>
// lane (rotation 260): `[[1,2], ["a","b"]]` unified on the first
// column's kind and the typed lane raw-read the Str column's
// pointers as I64 (bare pointer digits). Covers the typed decl
// read path (toplevel + fn body), the boxed-argv any-call path,
// and keeps the homogeneous / sentinel / struct-family shapes on
// their pre-existing lanes.
function g() {
  const m = [[1, 2], ["a", "b"]];
  console.log(m[1][0], m[0][1]);
  console.log(JSON.stringify(m));
  const s: string = m[1][1];
  console.log(s);
  const homo = [[1, 2], [3, 4]];
  console.log(homo[1][0] + homo[0][1]);
  const su = ["a", undefined];
  console.log(su[0], su[1]);
  const xy = [[1], [undefined, 2]];
  console.log(JSON.stringify(xy));
  const m3 = [["x"], [true, false], [1.5]];
  console.log(m3[0][0], m3[1][1], m3[2][0]);
  const st = [{ r: 2 }, { r: 3, s: 4 }];
  console.log(st[0].r + st[1].r);
}
g();
const t = [[1, 2], ["a", "b"]];
console.log(t[1][0]);
for (const r of t) {
  console.log(r.length);
}
const f: any = (x: any) => x[1][0];
console.log(f([[3, 4], ["c", "d"]]));
