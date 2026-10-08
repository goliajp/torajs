// The reflection built-ins read an interface value by the instance's
// own layout, whichever class (or literal) is behind it.
interface Named {
  name: string;
  hello(): string;
}
class U {
  id: number;
  name: string;
  constructor(n: string) {
    this.id = 3;
    this.name = n;
  }
  hello(): string {
    return "hi " + this.name;
  }
}
const n: Named = new U("a");
const l: Named = {
  name: "l",
  hello() {
    return "x";
  },
};
console.log(JSON.stringify(n), JSON.stringify(l));
console.log(Object.keys(n), Object.keys(l));
console.log(Object.values(n), Object.entries(n));
for (const k in n) console.log(k);
const arr: Named[] = [n, new U("b")];
console.log(JSON.stringify(arr));
const a: any = n;
console.log(Object.keys(a).join(","), a.hello());
