// Adapted from test262: language/statements/class/subclass/* —
// single-inheritance: subclass adds fields, parent fields inherited.
class Animal {
  legs: number;
  constructor(legs: number) {
    this.legs = legs;
  }
  describeLegs(): number { return this.legs; }
}

class Dog extends Animal {
  name: string;
  constructor(name: string) {
    super(4);
    this.name = name;
  }
  describeName(): string { return this.name; }
}

function check(): number {
  let d = new Dog("Rex");
  if (d.legs !== 4) { throw "#1"; }
  if (d.name !== "Rex") { throw "#2"; }
  if (d.describeLegs() !== 4) { throw "#3"; }
  if (d.describeName() !== "Rex") { throw "#4"; }
  return 0;
}
console.log(check());
