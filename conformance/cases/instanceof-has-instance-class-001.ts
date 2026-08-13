// a class that declares its own @@hasInstance decides for any operand
class Even {
  static [Symbol.hasInstance](x: any): boolean {
    return typeof x === "number" && x % 2 === 0;
  }
}
console.log("A1", (4 as any) instanceof Even);
console.log("A2", (5 as any) instanceof Even);
console.log("A3", 6 instanceof Even);
console.log("A4", ({} as any) instanceof Even);

// the handler sees `this` as the class object
class Tagged {
  static tag = 3;
  static [Symbol.hasInstance](x: any): boolean { return x === (this as any).tag; }
}
console.log("A5", (3 as any) instanceof Tagged);
console.log("A6", (4 as any) instanceof Tagged);

// an ordinary class keeps the hierarchy answer
class Animal {}
class Dog extends Animal {}
const d = new Dog();
console.log("G1", d instanceof Dog);
console.log("G2", d instanceof Animal);
console.log("G3", new Animal() instanceof Dog);
console.log("G4", ({} as any) instanceof Animal);

// a subclass of a class with a handler: the handler is inherited
class EvenSub extends Even {}
console.log("A7", (8 as any) instanceof EvenSub);
