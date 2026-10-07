// `await p` reads `p` first, then runs jobs until the promise settles;
// a job that rebinds `p` must not release the promise being awaited.

var p: Promise<boolean> = Promise.resolve().then(() => {
  p = Promise.resolve().then(() => false);
  return true;
});
console.log(await p);

var r: Promise<number> = Promise.resolve(1).then(v => { r = Promise.resolve(9); return v + 1; });
console.log(await r);

var values: number[] = [];
var w: Promise<boolean> = Promise.resolve().then(() => {
  w = Promise.resolve().then(() => {
    w = Promise.resolve().then(() => { values.push(3); return false; });
    values.push(2);
    return true;
  });
  values.push(1);
  return true;
});
let n = 0;
while (await w) { n++; }
console.log(values.join(","), n);

var a: any = Promise.resolve("x").then((s: string) => { a = Promise.resolve("y"); return s + "!"; });
console.log(await a);

