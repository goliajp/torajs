// Integration: build a CSV-shaped string from a number array using
// repeated `+= n` concat (which only became possible after both
// number→string coercion AND compound-assign on String slots landed).
function joinCsv(xs: number[]): string {
  let out: string = "";
  for (let i: number = 0; i < xs.length; i++) {
    if (i === 0) {
      out = "" + xs[i];
    } else {
      out += "," + xs[i];
    }
  }
  return out;
}

function check(): number {
  if (joinCsv([1, 2, 3, 4, 5]) !== "1,2,3,4,5") { throw "#1"; }
  if (joinCsv([42]) !== "42") { throw "#2"; }
  if (joinCsv([0, -1, 100]) !== "0,-1,100") { throw "#3"; }
  return 0;
}
console.log(check());
