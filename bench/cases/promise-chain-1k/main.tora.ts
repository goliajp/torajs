function add1(v: number): number { return v + 1; }

let p: Promise<number> = Promise.resolve(0);
for (let i: number = 0; i < 1000; i = i + 1) {
  p = p.then(add1);
}
console.log(await p);
