// String methods called on a substring view — a split element, a
// character read, a for-of variable — that have no view kernel of
// their own. The compiler refused the program outright
// ("unsupported Substr method"); the view is now copied out and the
// method asked of the copy.
function rows(s: string): string {
  const out: string[] = [];
  for (const row of s.split(";")) {
    const cells = row.split(",");
    out.push(cells.length + ":" + cells.join("|"));
  }
  return out.join(" ");
}
console.log(rows("1,2;3;a,b,c"));

const parts = ("ab-cd," + String(7)).split(",");
const first: string = parts[0];
console.log(first.split("-").join("+"), first.substr(1, 2), first.concat("!", parts[1]));
console.log(parts[1].normalize(), parts[0].isWellFormed(), parts[0].toWellFormed());
console.log(parts[0].toLocaleString(), parts[1].bold(), parts[1].anchor("x"));

const s = "hello" + String(1);
console.log(s.charAt(1).concat(s.charAt(2)), s[0].split(""));
for (const c of "xy") console.log(c.concat("z"), c.split("").length);

// the copy is released while the split product it produced lives on
function keep(n: number): string[] {
  const src = ("p" + n + "-q" + n + ",r").split(",");
  return src[0].split("-");
}
const kept = keep(4);
const churn: string[] = [];
for (let i = 0; i < 50; i++) churn.push("vvvvvvvvv" + i);
console.log(kept.join(" "));
