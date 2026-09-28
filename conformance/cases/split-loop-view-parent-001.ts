// A split loop whose string is itself a substring view — the inner
// `row.split(",")` of a nested split loop, or a character read. The
// loop's iterator reads its string as a plain one, and handed a view
// it walked the view's header as text: the cells printed as garbage.
function churn(): void {
  const j: string[] = [];
  for (let i = 0; i < 50; i++) j.push("vvvvvvvvv" + i);
}
function cells(s: string): string {
  const out: string[] = [];
  for (const row of s.split(";")) {
    for (const c of row.split(",")) out.push(c);
    churn();
  }
  return out.join("|");
}
console.log(cells("1,2;3" + ";4,5"));
console.log(cells("a;;b,c,"));

const s = "xy" + String(1);
for (const c of s.charAt(0).split("")) console.log(c);
