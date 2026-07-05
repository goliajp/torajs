// RFC 20260705 chunk 547 — the array-literal anchor-type probe used to
// lower the first non-empty element twice: double side-effect (step()
// fired twice, a[0] read the second call) and the first evaluation's
// owned result leaked. Elements must evaluate exactly once, in order.
let count = 0;
function step(): number {
  count = count + 1;
  return count;
}
let a = [step(), step(), 100];
console.log(a[0]);
console.log(a[1]);
console.log(a[2]);
console.log(count);
function tag(t: string): string {
  count = count + 10;
  return t;
}
let b = [tag("x"), tag("y")];
console.log(b[0]);
console.log(b[1]);
console.log(count);
let c = ["hello" + count, "w" + 9];
console.log(c[0]);
console.log(c[1]);
