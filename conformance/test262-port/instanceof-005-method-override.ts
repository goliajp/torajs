// Phase H.3.b — method override + virtual dispatch. Subclass redeclares
// the parent's method body; tr emits a `__dispatch_<method>` fn that
// walks the runtime class tag (via instanceof) deepest-first, falling
// through to the base owner. Polymorphic Animal[] holding Dog/Puppy
// dispatches to the override at the actual runtime type.
class Animal {
  kind: string;
  constructor(k: string) { this.kind = k; }
  greet(): string { return "Animal:" + this.kind; }
}
class Dog extends Animal {
  breed: string;
  constructor(k: string, b: string) { super(k); this.breed = b; }
  greet(): string { return "Dog:" + this.kind + ":" + this.breed; }
}
class Puppy extends Dog {
  weeks: number;
  constructor(k: string, b: string, w: number) { super(k, b); this.weeks = w; }
  greet(): string { return "Puppy:" + this.kind + ":" + this.breed + ":" + this.weeks.toString(); }
}
class Cat extends Animal {
  color: string;
  constructor(k: string, c: string) { super(k); this.color = c; }
  // Cat does NOT override greet — inherits Animal's body.
}

function check(): number {
  // Direct concrete dispatch — each variable typed as its actual class.
  let a = new Animal("generic");
  if (a.greet() !== "Animal:generic") { throw "#1: animal direct"; }

  let d = new Dog("dog", "lab");
  if (d.greet() !== "Dog:dog:lab") { throw "#2: dog direct override"; }

  let p = new Puppy("dog", "lab", 8);
  if (p.greet() !== "Puppy:dog:lab:8") { throw "#3: puppy direct override"; }

  let c = new Cat("cat", "black");
  // Cat doesn't override; inherits Animal's body.
  if (c.greet() !== "Animal:cat") { throw "#4: cat inherit base"; }

  // Polymorphic Animal[] — each element answers greet() per its actual
  // runtime class via __dispatch_greet.
  let zoo: Animal[] = [
    new Animal("zoo-animal"),
    new Dog("zoo-dog", "husky"),
    new Puppy("zoo-pup", "husky", 6),
    new Cat("zoo-cat", "ginger"),
  ];

  if (zoo[0].greet() !== "Animal:zoo-animal") { throw "#5: poly animal"; }
  if (zoo[1].greet() !== "Dog:zoo-dog:husky") { throw "#6: poly dog"; }
  if (zoo[2].greet() !== "Puppy:zoo-pup:husky:6") { throw "#7: poly puppy"; }
  if (zoo[3].greet() !== "Animal:zoo-cat") { throw "#8: poly cat-inherit"; }

  // Loop accumulates greetings — exercises dispatch in a tight loop.
  let acc: string = "";
  for (let i: number = 0; i < zoo.length; i = i + 1) {
    if (i > 0) acc = acc + "|";
    acc = acc + zoo[i].greet();
  }
  let want = "Animal:zoo-animal|Dog:zoo-dog:husky|Puppy:zoo-pup:husky:6|Animal:zoo-cat";
  if (acc !== want) { throw "#9: poly loop"; }

  return 0;
}
console.log(check());
