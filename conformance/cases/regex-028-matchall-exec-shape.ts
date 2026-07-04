// every matchAll element carries the full exec shape:
// index (UTF-16 code units) / input / groups
const s: string = "a1 b2 c3";
for (const m of s.matchAll(/([a-z])(\d)/g)) {
  console.log(m);
}
for (const m of "xx yy".matchAll(/y/g)) {
  console.log(m.index);
}
// named captures populate .groups on each element
for (const m of "k=1;j=2".matchAll(/(?<key>\w)=(?<val>\d)/g)) {
  console.log(m.index, m.groups.key, m.groups.val);
}
// non-ASCII haystack: index is code units, not transcoded bytes
const u: string = "世界 ab 世界 ab";
for (const m of u.matchAll(/ab/g)) {
  console.log(m.index);
}
