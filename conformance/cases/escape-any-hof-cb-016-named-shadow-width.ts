// A callback name a local binding shadows carries that binding's
// widths, not those of the top-level declaration spelled the same
function sq(v: number): number {
  return 1;
}

function viaLet(d: number[]): string {
  let sq = (v: number) => v + 1;
  return d.map(sq).join(",");
}
console.log(viaLet([1.5, 3]));

function viaParam(sq: (v: number) => number, d: number[]): string {
  return d.map(sq).join(",");
}
console.log(viaParam((v: number) => v + 1, [1.5, 2]));

function viaEach(d: number[]): number {
  let t = 0;
  let sq = (v: number) => {
    t = t + v;
  };
  d.forEach(sq);
  return t;
}
console.log(viaEach([0.5, 0.25]));

function viaFrom(d: number[]): string {
  let sq = (v: number) => v * 1;
  return Array.from(d, sq).join(",");
}
console.log(viaFrom([0.5, 2]));

function viaFilter(d: number[]): string {
  let sq = (v: number) => v / 3 > 1;
  return d.filter(sq).join(",");
}
console.log(viaFilter([2, 4.5, 6]));

function viaCapture(sq: (v: number) => number): (d: number[]) => string {
  return (d: number[]) => d.map(sq).join(",");
}
console.log(viaCapture((v: number) => v / 2)([1, 3]));

console.log([1, 2].map(sq).join(","), sq(7));
