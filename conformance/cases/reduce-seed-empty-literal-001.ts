// an empty array literal seed is typed by the accumulator the
// callback declares, as under an annotated binding
const ys = [1, 2, 3];
const r = ys.reduce((acc: number[], x: number) => { acc.push(x * 2); return acc }, []);
console.log(r.join(","), r.length, typeof r);

const q = ys.reduce((acc: number[], x: number) => { acc.push(x + 0.5); return acc }, [] as number[]);
console.log(q.join(","), q[0] + q[2]);

const w = ["a", "b", "c"].reduceRight((acc: string[], x: string) => { acc.push(x + x); return acc }, []);
console.log(w.join("|"));

function evens(xs: number[]): number[] {
  return xs.reduce((acc: number[], x: number) => {
    if (x % 2 === 0) acc.push(x);
    return acc;
  }, []);
}
const ev = evens([1, 2, 3, 4, 5, 6]);
ev.push(8);
console.log(ev.join(","), ev.length);

const c = ys.reduce((acc: number[], x: number) => acc.concat([x * 3]), []);
console.log(c.join(","));

let total = 0;
for (let i = 0; i < 2000; i++) {
  const e = ys.reduce((acc: number[], x: number) => { acc.push(x + i); return acc }, []);
  total += e[2];
}
console.log(total);
