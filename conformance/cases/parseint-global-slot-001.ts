// a top-level binding initialized by the builtin parsers holds a Number,
// and named functions read it through a number slot
const n = parseInt("3", 10);
const f = parseFloat("2.5");
const bad = parseInt("zz", 10);
const a = "ke" + n;
function useN(): number {
  return n + 1;
}
function useF(): number {
  return f * 2;
}
function isBad(): boolean {
  return bad !== bad;
}
function pushS(xs: string[]): void {
  xs.push(a);
}
let s = "";
for (let i = 0; i < 3; i++) {
  s = s + a;
}
const ys: string[] = [];
pushS(ys);
console.log(s, useN(), useF(), isBad(), ys[0], typeof n, typeof f);

function g(): void {
  const out: number[][] = [];
  const pair = [n, 1];
  out.push(pair);
  console.log(out[0][0] + out[0][1]);
}
g();

function h(): void {
  const os: { k: string }[] = [];
  const b = "of" + n;
  os.push({ k: b });
  console.log(os[0].k, os.length);
}
h();
