// A binding typed by an interface takes a class instance, then a
// literal, then another instance; containers of it keep each one's
// members.
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
let n: Named = new U("a");
console.log(n.hello());
n = {
  name: "lit",
  hello() {
    return "yo";
  },
};
console.log(n.hello(), n.name);
n = new U("b");
console.log(n.hello(), n.name);
const m = new Map<string, Named>();
m.set("k", new U("c"));
console.log(m.get("k")!.hello());
function maybe(f: boolean): Named | null {
  return f ? new U("d") : null;
}
const z = maybe(true);
if (z) console.log(z.hello());
console.log(maybe(false) === null);
