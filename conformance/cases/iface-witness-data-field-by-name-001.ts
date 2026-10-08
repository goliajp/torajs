// A data-only interface matches a class by member NAME, wherever the
// class lays the member out; an array of unrelated classes behind it
// reads each element's own slot.
interface Named {
  name: string;
}
class User {
  id: number;
  name: string;
  constructor(id: number, name: string) {
    this.id = id;
    this.name = name;
  }
}
class Item {
  name: string;
  price: number;
  constructor(n: string, p: number) {
    this.name = n;
    this.price = p;
  }
}
const u: Named = new User(7, "ann");
console.log(u.name);
const all: Named[] = [u, new Item("pen", 1.5), { name: "lit" }];
for (const n of all) console.log(n.name);
function rename(n: Named, to: string) {
  n.name = to;
}
rename(u, "bea");
rename(all[1], "ink");
for (const n of all) console.log(n.name);
interface V {
  v: number;
}
class A {
  v: number;
  constructor() {
    this.v = 1;
  }
}
class B {
  w: string;
  v: number;
  constructor() {
    this.w = "x";
    this.v = 2;
  }
}
const xs: V[] = [new A(), new B()];
for (const x of xs) console.log(x.v);
