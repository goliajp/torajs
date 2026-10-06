// a struct-array binding spelled `Array<A>` or through an alias, then
// assigned a literal with an `any` element, holds the dynamic layout
// from its declaration: locals, top-level bindings and a top-level one
// a named function reads

interface A { x: number; y: string }
type AS = A[];
const j: any = JSON.parse('{"x": 6, "y": "six"}');
let g1: Array<A> = [{ x: 0, y: "o" }];
g1 = [{ x: 1, y: "a" }, j];
console.log(g1[1].x, g1[1].y, g1[1] === j);
let g2: AS = [{ x: 0, y: "o" }];
g2 = [{ x: 2, y: "b" }, j];
console.log(g2[1].x, g2[1].y, g2.length);
function loc(v: any) {
  let l1: Array<A> = [];
  l1 = [v, { x: 3, y: "c" }];
  let l2: AS = [];
  l2 = [{ x: 4, y: "d" }, v];
  console.log(l1[0].y, l2[1].y, l1.map((e) => e.x).join(","));
}
loc({ x: 9, y: "nine" });
loc(5);
let topRead: A[] = [];
function readIt(): string { return topRead[1].y; }
topRead = [{ x: 1, y: "t" }, j];
console.log(readIt());
