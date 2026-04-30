// Adapted from test262: string-typed class fields. Verifies tr's
// String slot in the class layout reads + concats correctly.
class Greeting {
  prefix: string;
  name: string;
  constructor(p: string, n: string) {
    this.prefix = p;
    this.name = n;
  }
  full(): string { return this.prefix + this.name; }
}

function check(): number {
  let g = new Greeting("hello, ", "world");
  if (g.prefix !== "hello, ") { throw "#1"; }
  if (g.name !== "world") { throw "#2"; }
  if (g.full() !== "hello, world") { throw "#3"; }
  return 0;
}
console.log(check());
