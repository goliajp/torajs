// A typed promise receiver hands a raw handler one argument; a handler
// declaring more parameters must see the rest as undefined.
function two(a: number, b?: number): number {
  return b === undefined ? a + 1000 : a + b;
}
function one(a: number): number {
  return a + 1;
}
const p: Promise<number> = Promise.resolve(6);
p.then(two).then((v) => console.log("two", v));
p.then(one).then((v) => console.log("one", v));
const r: Promise<number> = Promise.reject(3);
r.catch(two).then((v) => console.log("catch two", v));
const xs: number[] = [1, 2];
console.log(xs.map(one).join(","));
