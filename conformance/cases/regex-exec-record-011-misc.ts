// The remaining accepted shapes: `m.input`, a temporary subject, a
// miss read un-narrowed (TypeError), `m.index` in a nested block, and
// two record bindings alive at once.

function temp(s: string): string {
  const re = /(b+)/;
  let m = re.exec(s + "bb");
  return m === null ? "none" : m.input + "|" + m[1] + "|" + m.index;
}
function onMiss(s: string): string {
  const re = /(q)/;
  const m = re.exec(s);
  try {
    return "index " + m.index;
  } catch (e) {
    return "caught " + (e instanceof TypeError);
  }
}
function nested(s: string): number {
  const re = /(\d)/g;
  let acc = 0;
  let m = re.exec(s);
  while (m !== null) {
    {
      const k = m.index * 10;
      acc = acc + k + m[1].length;
    }
    m = re.exec(s);
  }
  return acc;
}
function two(s: string): string {
  const a = /(\d)/g;
  const b = /([a-z])/g;
  let x = a.exec(s);
  let y = b.exec(s);
  let out = "";
  while (x !== null && y !== null) {
    out = out + x[1] + y[1] + (x.index + y.index) + ",";
    x = a.exec(s);
    y = b.exec(s);
  }
  return out;
}
console.log(temp("aab"), temp("zzz"));
console.log(onMiss("abc"), onMiss("q"));
console.log(nested("1a2b3"));
console.log(two("a1b2c3"), two("9"));
