// a conditional arm that is a comma expression ending in a string index
let calls = 0;
function f(): void {
  calls++;
}
function pick(s: string, i: number, c: boolean): string {
  return c ? (f(), s[i]) : s;
}
const s = "abcdef";
let total = 0;
let last = "";
for (let k = 0; k < 300000; k++) {
  const r = pick(s, k % 6, k % 3 !== 0);
  total += r.length;
  last = r;
}
console.log(total, last, calls);
const t = pick("xyz", 1, true);
console.log(t, t.length, pick("xyz", 1, false));
