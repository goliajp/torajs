// Two more consumers of the same ownership predicate family: the
// assignment target and an array-literal element slot. Both judged the
// raw expression, so an `As` wrapper made them read a binding as "a
// fresh value that transfers" and store its pointer bare -- the source's
// scope drop then released a cell the survivor still pointed at. The
// uncast form is correct in both, which is what makes the shape easy to
// miss. The churn loop reuses the freed page so a stale read shows up as
// the wrong text.
function aliased(): string {
  const src = "abcdefghijklmnopqrstuvwxyz" + "0123456789";
  let dst = "seed";
  dst = src as any;
  return dst;
}

function boxedAlias(): any {
  const a: any = "the quick brown fox jumps over" + " the lazy dog";
  let dst: any = "seed";
  dst = a as any;
  return dst;
}

function elemAlias(): string[] {
  const src = "pack my box with five dozen" + " liquor jugs";
  return [src as string];
}

const one = aliased();
const two = boxedAlias();
const three = elemAlias();

for (let i = 0; i < 400; i++) {
  const junk = "q" + i + "padpadpadpadpadpadpadpadpadpad";
  if (junk === "never") console.log(junk);
}

console.log(one);
console.log(two);
console.log(three[0]);
