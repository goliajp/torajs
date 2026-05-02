function rowLen(line: string): number {
  let parts: string[] = line.split(",");
  let total: number = 0;
  for (let i: number = 0; i < parts.length; i = i + 1) {
    let t: string = parts[i].trim();
    total = total + t.length;
  }
  return total;
}

let total: number = 0;
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  total = total + rowLen("  alpha , beta , gamma , delta , epsilon , zeta  ");
}
console.log(total);
