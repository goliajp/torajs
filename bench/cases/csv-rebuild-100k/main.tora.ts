function rebuild(line: string): number {
  let parts: string[] = line.split(",");
  let total: number = 0;
  for (let i: number = 0; i < parts.length; i = i + 1) {
    let s: string = parts[i] + "|";
    // sample a byte so the concat result actually escapes to a use
    // that bun can't skip via ConsString lazy-evaluation
    total = total + s.charCodeAt(0);
  }
  return total;
}

let total: number = 0;
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  total = total + rebuild("alpha,beta,gamma,delta,epsilon,zeta");
}
console.log(total);
