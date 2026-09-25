// a string known to be ascii stops being known the moment a non-ascii
// piece is appended onto it in place; a regexp that reads it as utf-16
// units afterwards must see the appended character, not its byte.
function grow(n: number): string {
  let out = "";
  for (let i = 0; i < n; i++) {
    let acc = "ab" + i.toString();
    acc = acc + "é";
    const m = acc.match(/é/);
    const w = acc.match(/b\d+é/);
    out = out + (m !== null ? m[0].length : -1) + ":" + (w !== null ? w[0].length : -1) + " ";
  }
  return out;
}
console.log(grow(3));

function keep(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    let acc = "ab" + i.toString();
    acc = acc + "cd";
    const m = acc.match(/b\d+c/);
    if (m !== null) total = total + m[0].length;
  }
  return total;
}
console.log(keep(12));

function wide(): string {
  let acc = "x" + (7).toString();
  acc = acc + "中";
  const m = acc.match(/\d中/);
  return m !== null ? "" + m[0].length : "null";
}
console.log(wide());
