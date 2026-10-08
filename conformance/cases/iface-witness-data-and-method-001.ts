// An interface with data members and a method. The two classes lay the
// data out in different orders (and carry members the interface does
// not name); reads, writes and compound writes through the interface
// reach each instance's own fields.
interface Animal {
  name: string;
  legs: number;
  speak(): string;
}
class Dog {
  owner: string;
  name: string;
  legs: number;
  constructor(n: string) {
    this.owner = "ann";
    this.name = n;
    this.legs = 4;
  }
  speak(): string {
    return this.name + " woofs (" + this.owner + ")";
  }
}
class Bird {
  legs: number;
  wings: number;
  name: string;
  constructor(n: string) {
    this.legs = 2;
    this.wings = 2;
    this.name = n;
  }
  speak(): string {
    return this.name + " tweets";
  }
}
const zoo: Animal[] = [
  new Dog("rex"),
  new Bird("tweety"),
  {
    name: "snake",
    legs: 0,
    speak() {
      return "hiss";
    },
  },
];
let legs = 0;
for (const a of zoo) {
  legs += a.legs;
  console.log(a.name, a.legs, a.speak());
}
console.log("legs", legs);
zoo[1].name = "polly";
zoo[1].legs += 1;
zoo[0].legs = zoo[0].legs * 2.5;
for (const a of zoo) console.log(a.name, a.legs, a.speak());
function describe(a: Animal): string {
  return a.name + ":" + a.legs;
}
console.log(describe(new Bird("kiwi")), describe(zoo[0]));
const { name, legs: n } = zoo[1];
console.log(name, n);
