// a ternary / nullish / logical join over borrowed strings, used as a
// concat operand: the join hands the binding's own value through, and
// the binding must keep it (its length stays the same every round)
const h = "z".repeat(10);
const g = h + h;
const maybe: string | undefined = g.length > 0 ? g : undefined;
function f(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    total += ((i === n - 1 ? g : "ab") + "|").length;
    total += ((i === n - 1 ? g : g) + "|").length;
    total += ("|" + (i === n - 1 ? g : "ab")).length;
    total += ((maybe ?? "x") + "|").length;
    total += ((i > 5 && g) || g).length;
    total += (((i > 5 && g) || g) + "!").length;
  }
  return total;
}
for (let r = 0; r < 3; r++) console.log(f(2), g.length, g === h + h);
function mk(i: number): string {
  return "m" + i;
}
let acc = "";
for (let i = 0; i < 4; i++) acc += (i % 2 === 0 ? mk(i) : g) + ",";
console.log(acc.length, g.length);
