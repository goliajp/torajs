function walk(s: string): void {
  let n: number = 0;
  let acc: string = "";
  for (let t of s.split(" ")) {
    n = n + 1;
    acc = acc + "[" + t + "]";
  }
  console.log(n, acc);
}
walk("3 4 + 2 * 5 +");
walk("");
walk(" ");
walk("a ");
walk(" b");
walk("a  b");
walk("hello");
walk("日本 語 テスト");
walk("ab cd  ef ");
function walk2(s: string): void {
  let n: number = 0;
  for (let t of s.split("、")) {
    n = n + 1;
    console.log(t);
  }
  console.log(n);
}
walk2("あ、い、う");
function walkComma(s: string): void {
  let out: string = "";
  for (let t of s.split(",")) {
    out = out + t + "|";
  }
  console.log(out);
}
walkComma("1,2,,3,");
