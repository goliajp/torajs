// Methods an interface inherits through `extends` are called through
// the derived interface and, after a widening, through the base one.
interface Named {
  name(): string;
}
interface Greeter extends Named {
  greet(other: string): string;
}
class En {
  n: string;
  constructor(n: string) {
    this.n = n;
  }
  name(): string {
    return this.n;
  }
  greet(o: string): string {
    return "hello " + o + ", I am " + this.n;
  }
}
class Jp {
  id: number;
  n: string;
  constructor(n: string) {
    this.id = 1;
    this.n = n;
  }
  name(): string {
    return this.n + "-san";
  }
  greet(o: string): string {
    return o + " konnichiwa";
  }
}
const gs: Greeter[] = [new En("bob"), new Jp("taro")];
for (const g of gs) console.log(g.name(), g.greet("amy"));
const ns: Named[] = gs;
for (const n of ns) console.log(n.name());
function nameOf(n: Named): string {
  return n.name();
}
console.log(nameOf(gs[0]), nameOf(new Jp("hana")));
