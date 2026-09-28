// An element of a split product read through the any lane — an array
// iterator step or an index read on an `any` receiver — outlives the
// split array. The read used to hand out the substring view itself,
// whose storage belongs to the split array: once that array died (or
// a later allocation reused the bytes) the value printed another
// string's text, and a generator yielding the nested split's cells
// answered "done" for its last value.
function churn(): void {
  const j: string[] = [];
  for (let i = 0; i < 50; i++) j.push("vvvvvvvvv" + i);
}

function* rowsCells(s: string) {
  const rows = s.split(";");
  for (const row of rows) {
    const cells = row.split(",");
    for (const cell of cells) yield row + ":" + cell;
  }
}
function* cellsOnly(s: string) {
  const rows = s.split(";");
  for (const row of rows) {
    const cells = row.split(",");
    for (const cell of cells) yield cell;
  }
}
function* firstCell(s: string) {
  const rows = s.split(";");
  for (const row of rows) {
    const cells = row.split(",");
    yield cells[0];
  }
}
console.log([...rowsCells("1,2;3")].join("|"));
console.log([...rowsCells("a,b;c,d;e")].join("|"));
console.log([...cellsOnly("1,2;3")].join("|"));
console.log([...firstCell("1,2;3")].join("|"));

const it = cellsOnly("1,2;3");
const a = it.next().value;
churn();
const b = it.next().value;
churn();
console.log(a, b, it.next().value, it.next().done);

function stepValue(n: number): any {
  const s = "a" + n + ",b" + n + ";c";
  const rows: any = s.split(";");
  return rows[Symbol.iterator]().next().value;
}
function anyIndex(n: number): any {
  const c: any = ("a" + n + ",b" + n).split(",");
  return c[1];
}
function anyIndexTwice(n: number): string {
  const c: any = ("a" + n + ",b" + n).split(",");
  const x: any = c[0];
  churn();
  const y = String(c[0]);
  return x + "/" + y;
}
const r1 = stepValue(4);
const r2 = anyIndex(5);
churn();
console.log(r1, r2, anyIndexTwice(6));
