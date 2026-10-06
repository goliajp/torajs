// a matcher state that loops on all but a few units skips them a word
// at a time; the first leaving unit can sit at any lane of a word, in
// the tail past the last whole word, or nowhere. over 16-bit units a
// word is four units, and under u a pair never splits.
const greedy = /a.+c/s;
const lazy = /a.+?c/s;
const plain = /a.+c/;
const negated = /a[^c]*c/;
const greedyU = /a.+c/su;
const neg3 = /x[^yz\n]*y/;

function at(s: string, re: RegExp): string {
  const m = re.exec(s);
  return m === null ? "-" : m.index + ":" + m[0].length;
}

function row(s: string): string {
  return [greedy, lazy, plain, negated, greedyU, neg3].map((re) => at(s, re)).join(" ");
}

const fills = ["b", "\n", "日", "😀", "\r"];
for (const fill of fills) {
  const lines: string[] = [];
  for (let n = 0; n <= 20; n++) {
    let body = "";
    for (let k = 0; k < n; k++) body += k % 3 === 2 ? fill : "q";
    lines.push(row("pre a" + body));
    for (let c = 0; c <= n; c++) {
      const s = "pre a" + body.slice(0, c) + "c" + body.slice(c) + " post";
      lines.push(row(s));
    }
    lines.push(row("a" + body + "c" + body + "c" + body));
    lines.push(row("x" + body + "y" + body + "zy"));
  }
  let h = 0;
  for (const l of lines) for (let i = 0; i < l.length; i++) h = (h * 31 + l.charCodeAt(i)) % 1000000007;
  console.log(JSON.stringify(fill), lines.length, h);
  console.log(lines.slice(0, 3).join(" | "));
  console.log(lines.slice(-3).join(" | "));
}
console.log(row("pre a\nmiddle\nc post 12345"));
console.log(row("a\ud83dc a\ude00c"));
console.log(row("xyz xqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqy"));
