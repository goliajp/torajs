// Adapted from test262: language/expressions/arrow-function/* — value-shape
// capture of an outer binding. (torajs captures by value; mutating the
// outer binding after closure creation does not affect the captured copy.)
function makeAdder(x: number): (y: number) => number {
  return (y: number): number => x + y;
}

function check(): number {
  let add10 = makeAdder(10);
  let add20 = makeAdder(20);
  if (add10(5) !== 15) { throw "#1"; }
  if (add20(5) !== 25) { throw "#2"; }
  if (add10(0) !== 10) { throw "#3"; }
  if (add20(100) !== 120) { throw "#4"; }
  return 0;
}
console.log(check());
