// a binding declared without an annotation, in a program where another
// function declares the same name with one, takes the type its
// initializer gives it; the arrows it receives are typed from that type
class Pt {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
function make(n: number): Pt[] {
  const ps: Pt[] = [];
  for (let i = 0; i < n; i++) ps.push(new Pt(i));
  return ps;
}
function labels(n: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < n; i++) out.push("l" + i);
  return out;
}
function annotated(n: number): number {
  const ps: Pt[] = make(n);
  return ps.map((p) => p.x * 2).reduce((a, b) => a + b, 0);
}
// same type as the annotated one
function sameType(n: number): string {
  const ps = make(n);
  const xs = ps.filter((p) => p.x % 2 === 0).map((p) => p.x + 1);
  return xs.join(",");
}
// another type under the same name
function otherType(n: number): string {
  const ps = labels(n);
  return ps.map((p) => p.toUpperCase() + p.length).join(",");
}
// a literal initializer inside a try block
function inTry(n: number): number {
  try {
    const ps = [n, n + 1, n + 2];
    return ps.map((p) => p * p).reduce((a, b) => a + b, 0);
  } finally {
    console.log("done");
  }
}
// a parameter named like a module function is not that function
function rebound(make: (n: number) => string[]): string {
  const ps = make(3);
  return ps.map((p) => p + "!").join("");
}
console.log(annotated(5));
console.log(sameType(5));
console.log(otherType(3));
console.log(inTry(2));
console.log(rebound(labels));
