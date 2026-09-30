// charCodeAt on a view taken by slice / substring, Latin-1 and UTF-16
// parents, in and out of range, and under a proven loop guard.
const lat = "hello, world";
const wide = "héllo 中文 😀!";

const v1 = lat.slice(7);
const v2 = wide.slice(6, 9);
const v3 = wide.substring(9);
console.log(v1, v1.length, v1.charCodeAt(0), v1.charCodeAt(4), v1.charCodeAt(5), v1.charCodeAt(-1));
console.log(v2.length, v2.charCodeAt(0), v2.charCodeAt(1), v2.charCodeAt(2), v2.charCodeAt(3));
console.log(v3.length, v3.charCodeAt(0), v3.charCodeAt(1), v3.charCodeAt(2), v3.charCodeAt(3), v3.charCodeAt(4));
console.log(v1.charCodeAt(1.5), v2.charCodeAt(NaN), v3.charCodeAt(Infinity), v1.charCodeAt());

function sum(s: string): number {
  let t = 0;
  for (let i = 0; i < s.length; i = i + 1) t = t + s.charCodeAt(i);
  return t;
}
console.log(sum(v1), sum(v2), sum(v3), sum(lat.slice(0, 0)));

for (const part of "ab cd éf".split(" ")) {
  let line = "";
  for (let i = 0; i < part.length; i = i + 1) line = line + part.charCodeAt(i) + " ";
  console.log(line, part.charCodeAt(part.length));
}
