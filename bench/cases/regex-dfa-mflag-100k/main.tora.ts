let total: number = 0;
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  let s: string = "pre\nline-text\nend " + i.toString();
  let m: string[] | null = s.match(/^line/m);
  if (m !== null) total = total + m[0].length;
}
console.log(total);
