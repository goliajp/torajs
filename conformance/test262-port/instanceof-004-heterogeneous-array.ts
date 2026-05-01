// Adapted from test262: instanceof + class hierarchy with runtime
// dispatch. Phase H.1 lands the per-instance class tag in the object
// header; Phase H.2 lifts the structural-equality typecheck to allow
// `Sub` instances in `Base[]` slots (struct prefix subtyping). Together
// they enable real polymorphic arrays: every element answers
// `instanceof` against its actual runtime class, not its static slot.
class Animal { kind: string; constructor(k: string) { this.kind = k; } }
class Dog extends Animal { breed: string; constructor(k: string, b: string) { super(k); this.breed = b; } }
class Puppy extends Dog { weeks: number; constructor(k: string, b: string, w: number) { super(k, b); this.weeks = w; } }
class Cat extends Animal { color: string; constructor(k: string, c: string) { super(k); this.color = c; } }

function check(): number {
  // Heterogeneous Animal[] holding mixed concrete types. Animal is
  // listed first so the inferred element type is Animal; Dog/Puppy/Cat
  // widen to it via struct-prefix subtyping.
  let zoo: Animal[] = [
    new Animal("generic"),
    new Dog("dog", "lab"),
    new Puppy("dog", "lab", 8),
    new Cat("cat", "black"),
  ];

  // Every element is an Animal (transitive instanceof up the chain).
  for (let i: number = 0; i < zoo.length; i = i + 1) {
    if (!(zoo[i] instanceof Animal)) { throw "#1: all are animals"; }
  }

  // The Animal slot itself is just an Animal, not a Dog/Puppy/Cat.
  if (zoo[0] instanceof Dog)   { throw "#2: a0-Dog false-positive"; }
  if (zoo[0] instanceof Puppy) { throw "#3"; }
  if (zoo[0] instanceof Cat)   { throw "#4"; }

  // The Dog slot is a Dog and an Animal but not Puppy/Cat.
  if (!(zoo[1] instanceof Dog))    { throw "#5"; }
  if (!(zoo[1] instanceof Animal)) { throw "#6"; }
  if (zoo[1] instanceof Puppy)     { throw "#7: dog isn't puppy"; }
  if (zoo[1] instanceof Cat)       { throw "#8"; }

  // The Puppy slot is a Puppy AND Dog AND Animal.
  if (!(zoo[2] instanceof Puppy))  { throw "#9"; }
  if (!(zoo[2] instanceof Dog))    { throw "#10: puppy is dog"; }
  if (!(zoo[2] instanceof Animal)) { throw "#11: puppy is animal"; }
  if (zoo[2] instanceof Cat)       { throw "#12: cross-tree neg"; }

  // The Cat slot — sibling of Dog, both Animal but not each other.
  if (!(zoo[3] instanceof Cat))    { throw "#13"; }
  if (!(zoo[3] instanceof Animal)) { throw "#14"; }
  if (zoo[3] instanceof Dog)       { throw "#15: sibling neg"; }
  if (zoo[3] instanceof Puppy)     { throw "#16"; }

  // Counts of each concrete type by walking the array — exercises the
  // runtime tag read in a real loop.
  let dogs: number = 0;
  let puppies: number = 0;
  let cats: number = 0;
  for (let i: number = 0; i < zoo.length; i = i + 1) {
    if (zoo[i] instanceof Puppy) { puppies = puppies + 1; }
    if (zoo[i] instanceof Dog)   { dogs = dogs + 1; }
    if (zoo[i] instanceof Cat)   { cats = cats + 1; }
  }
  // Puppy is also a Dog, so dog count is 2.
  if (dogs !== 2)    { throw "#17: dog count"; }
  if (puppies !== 1) { throw "#18: puppy count"; }
  if (cats !== 1)    { throw "#19: cat count"; }

  return 0;
}
console.log(check());
