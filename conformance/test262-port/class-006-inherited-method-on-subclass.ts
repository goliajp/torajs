// Adapted from test262: subclass instances dispatch parent's methods.
// The parent's method is keyed on its own class in the dispatch table;
// when called via a subclass instance it still works because the
// subclass struct's prefix is the parent's struct.
class Animal {
  name: string;
  constructor(name: string) { this.name = name; }
  greet(): string { return this.name; }
}

class Dog extends Animal {
  breed: string;
  constructor(name: string, breed: string) {
    super(name);
    this.breed = breed;
  }
}

function check(): number {
  let d = new Dog("Rex", "Lab");
  if (d.greet() !== "Rex") { throw "#1"; }
  if (d.name !== "Rex") { throw "#2"; }
  if (d.breed !== "Lab") { throw "#3"; }
  return 0;
}
console.log(check());
