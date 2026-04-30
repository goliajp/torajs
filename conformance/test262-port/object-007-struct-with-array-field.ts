// Adapted from test262: struct with an array-typed field. Read access
// works through the field; mutating the array via field-of-struct
// member call (e.g. `b.items.push(…)`) isn't supported in v0 yet —
// the workaround is to construct the array fully at the literal.
type Bag = { name: string, items: number[] };

function check(): number {
  let b: Bag = { name: "fruits", items: [10, 20, 30, 40] };
  if (b.name !== "fruits") { throw "#1"; }
  if (b.items.length !== 4) { throw "#2"; }
  if (b.items[0] !== 10) { throw "#3"; }
  if (b.items[3] !== 40) { throw "#4"; }
  let sum: number = 0;
  for (let i: number = 0; i < b.items.length; i = i + 1) {
    sum = sum + b.items[i];
  }
  if (sum !== 100) { throw "#5"; }
  return 0;
}
console.log(check());
