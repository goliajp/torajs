// an empty array literal seed takes the accumulator parameter's lane,
// not the callback's return lane, when the two differ
const r = [1].reduce((acc: any[], x: number) => { acc.push("s"); console.log(acc[0]); return [x]; }, []);
console.log(r.join(","), r.length);

const m = [1, 2, 3].reduce((acc: any[], x: number) => {
  acc.push(x === 1 ? "t" : x);
  console.log(acc.join("/"));
  return [x * 2];
}, [] as any[]);
console.log(m.join(","));

const o = ["a", "b"].reduceRight((acc: any[], x: string) => { acc.push(x === "b" ? 1.5 : "c"); return [x]; }, []);
console.log(o.join(","), typeof o[0]);

// a reducer that reads `this` carries a leading receiver slot
const t = [4, 5, 6].reduce(function (acc: number[], x: number) {
  if (this !== undefined) acc.push(-1);
  acc.push(x * 10);
  return acc;
}, []);
console.log(t.join(","), t.length);

const u = [7].reduce(function (acc: any[], x: number) {
  if (this !== undefined) acc.push(-1);
  acc.push("u");
  console.log(acc[0]);
  return [x];
}, []);
console.log(u.join(","));
