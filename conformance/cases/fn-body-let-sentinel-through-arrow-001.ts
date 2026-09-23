// The fall-through table composes over a binding declared in a
// function body: the arrow hands back what the binding holds, and the
// enclosing function hands back what the arrow returned.
const zs: number[] = [1, 2, 3];

function outer(): number {
  const u = zs[9];
  const give = () => u;
  return give();
}

console.log(typeof outer(), outer() === undefined);
