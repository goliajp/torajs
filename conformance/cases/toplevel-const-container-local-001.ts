// a top-level `const` container with no named-fn reader lives in main;
// one a named function reads stays global. every use shape below must
// answer the same either way.
const xs: number[] = [];
for (let i = 0; i < 10; i = i + 1) xs.push(i * i);
let s = 0;
for (let i = 0; i < xs.length; i = i + 1) s = s + xs[i];
console.log(s, xs.length);

function total(a: number[]): number {
  let t = 0;
  for (const v of a) t = t + v;
  return t;
}
console.log(total(xs));

const pt = { x: 1, y: 2 };
pt.x = pt.x + 40;
console.log(pt.x, pt.y);

const tags: string[] = ["a"];
const addTag = (t: string): number => tags.push(t);
addTag("b");
addTag("c");
console.log(tags.join(","));

const seen: number[] = [];
function note(v: number): void {
  seen.push(v);
}
note(3);
note(4);
console.log(seen.length, seen[1]);

const sq = (v: number): number => v * v;
console.log(xs.map(sq).slice(0, 3).join(" "));

while (xs.length > 7) xs.pop();
console.log(xs.join(" "));
