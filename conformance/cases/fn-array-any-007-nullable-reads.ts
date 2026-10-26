// An element of a function array whose element type admits `undefined`,
// read after a truthiness check: a callback's element parameter and a
// function parameter handed the element answer what the stored `any`
// function returns.
type F = (x: number) => number;
const q: any = (x: any) => x / 4;
const tag: any = (x: any) => "g" + x;
const hs: (F | undefined)[] = [(x: number) => x + 1, undefined];
hs[1] = tag;
hs.push(q);
console.log(hs.map((h) => (h ? h(2) : -1)).join(","));
hs.forEach((h) => {
  if (h) console.log(h(3));
});
for (const h of hs) console.log(h ? h(5) : 0);
function run(h: F | undefined) {
  if (h) console.log(h(6));
}
run(hs[1]);
run(hs[2]);
