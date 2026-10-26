// A function array whose element type admits `undefined` takes an
// `any` function through `push`, an element write and a literal, and a
// call through the element answers what the stored function returns.
type F = (x: number) => number;
const q: any = (x: any) => x / 4;
const tag: any = (x: any) => "g" + x;
const hs: (F | undefined)[] = [(x: number) => x + 1, undefined];
hs.push(q);
hs[1] = tag;
for (const h of hs) if (h) console.log(h(3));
const ls: (F | undefined)[] = [tag, undefined, q];
for (let i = 0; i < ls.length; i++) {
  const f = ls[i];
  if (f !== undefined) console.log(f(2));
}
console.log(ls.length);
