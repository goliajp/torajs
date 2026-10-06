// after a failed attempt the scan skips a word at a time to the next
// unit an attempt can begin on: the first attempt is never skipped, a
// begin unit can sit at any lane or in the tail, and an attempt that
// starts on it can still fail (`hellx` before `hello`).
const fold = /hello/i;
const three = /[xyz]q/;
const dot = /a.+c/s;
const pairU = /\u{1f600}x/u;

function at(s: string, re: RegExp): string {
  const m = re.exec(s);
  return m === null ? "-" : m.index + ":" + m[0].length;
}

function row(s: string): string {
  return [fold, three, dot, pairU].map((re) => at(s, re)).join(" ");
}

const fills = ["-", "日", "😀", "x", "H"];
for (const fill of fills) {
  const lines: string[] = [];
  for (let n = 0; n <= 20; n++) {
    let pad = "";
    for (let k = 0; k < n; k++) pad += k % 4 === 3 ? fill : ".";
    lines.push(row(pad + "HeLLo"));
    lines.push(row(pad + "hellx" + pad + "hello"));
    lines.push(row(pad + "xx" + pad + "zq"));
    lines.push(row(pad + "a" + pad + "c"));
    lines.push(row(pad + "😀" + pad + "😀x"));
    lines.push(row(pad));
  }
  let h = 0;
  for (const l of lines) for (let i = 0; i < l.length; i++) h = (h * 31 + l.charCodeAt(i)) % 1000000007;
  console.log(JSON.stringify(fill), lines.length, h);
  console.log(lines.slice(-6).join(" | "));
}

// the skip also runs between the matches of a global walk, from a
// lastIndex inside a word, and over a slice of a longer string
const g = "aa hello bb HELLO cc hello dd".replace(/hello/gi, "<$&>");
console.log(g);
const all = /hello/g;
let idx = "";
while (all.test("x hello y hello z")) idx += all.lastIndex + ",";
console.log(idx);
const re = /hello/g;
re.lastIndex = 3;
console.log(re.test("hello hello hello"), re.lastIndex);
const sticky = /hello/y;
sticky.lastIndex = 6;
console.log(sticky.test("hello hello"), sticky.lastIndex);
const big = "0123456789 prefix ... hello there hello";
console.log(big.slice(5).search(/hello/), big.slice(25).search(/hello/i));
