function local() {
  const a: any[] = [1, "x"];
  const n = a.unshift(5);
  console.log(n, a[0], a[1], a[2]);
  a.unshift("z");
  console.log(a);
  const s = "he" + "ap";
  a.unshift(s);
  console.log(a[0]);
}
local();
const g: any[] = [1, "x"];
const gn = g.unshift(true);
console.log(gn, g);
g.unshift(2.5);
console.log(g);
