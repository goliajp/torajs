// a non-global, non-sticky match whose result is only null-tested or
// measured searches once from 0: a literal pattern by memchr, any
// other by the regexp's dfa. both must answer what the full walk
// answers, and a haystack that is not ascii must still be read right.
function lens(s: string): string {
  const lit = s.match(/wor/);
  const miss = s.match(/zq/);
  const caps = s.match(/(\w)(\w+)/);
  const alt = s.match(/l+o|d$/);
  return (
    (lit !== null ? lit[0].length : -1) +
    "," +
    (miss === null ? "n" : "y") +
    "," +
    (caps !== null ? caps[0].length : -1) +
    "," +
    (alt !== null ? alt[0].length : -1)
  );
}
console.log(lens("hello world"));
console.log(lens("héllo wörld"));
console.log(lens("中文 world"));
console.log(lens(""));

function built(): number {
  let total = 0;
  for (let i = 0; i < 20; i++) {
    const s = "row " + i.toString();
    const a = s.match(/\d+$/);
    if (a !== null) total = total + a[0].length;
    const b = s.match(/row/);
    if (b) total = total + 100;
    const c = ("é" + i.toString()).match(/\d/);
    if (c !== null) total = total + 1000 * c[0].length;
  }
  return total;
}
console.log(built());
