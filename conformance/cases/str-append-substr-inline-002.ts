// Edges of the inline substring append: a piece cut from the
// accumulator itself, pieces landing exactly on the capacity edge at
// 16 and 17 bytes, and empty pieces of a two-byte parent.
let acc = "";
for (let i = 0; i < 4; i++) acc = acc + "ab,";
for (const p of acc.split(",")) acc = acc + p;
console.log(acc);
for (let n = 1; n <= 12; n++) {
  let o = "";
  const parts = ("q".repeat(16) + "," + "r".repeat(17)).split(",");
  for (let i = 0; i < n; i++) o = o + parts[i & 1];
  console.log(n, o.length, o.slice(-3));
}
function joined(s: string): string {
  let out = "";
  for (const p of s.split(",")) out = out + p + "|";
  return out;
}
console.log(joined("a中,,b"), joined(",中"), joined(""));
