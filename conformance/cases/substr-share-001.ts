// Sharing a split element — capturing it in a closure, or assigning it
// to another string binding. The element is a view whose storage is the
// split array's, and a refcount taken on it is never read by its drop:
// the closure read another string's bytes once the array was gone, and
// the assignment's share-then-release returned the source string's
// reference twice, so the source itself was freed while still in use.
function churn(): void {
  const j: string[] = [];
  for (let i = 0; i < 50; i++) j.push("vvvvvvvvv" + i);
}

function capture(n: number): () => string {
  const d = ("k" + n + ",z").split(",");
  const x = d[0];
  return () => x;
}
const h = capture(7);
churn();
console.log(h(), h().length);

function alias(n: number): string {
  const s = "m" + n + ",nn";
  const d = s.split(",");
  let r: string = "";
  for (let k = 0; k < 5; k++) r = d[0];
  churn();
  return s + "/" + r;
}
let out = "";
for (let i = 0; i < 3; i++) out += alias(i) + " ";
console.log(out);
