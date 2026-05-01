// Adapted from test262 instanceof + class hierarchy. tr's
// desugar_classes records the parent map (`class B extends A`); the
// compile-time instanceof walker traverses it so a Sub instance is
// instanceof every ancestor class up the chain.
class Animal { kind: string; constructor(k: string) { this.kind = k; } }
class Dog extends Animal { breed: string; constructor(k: string, b: string) { super(k); this.breed = b; } }
class Puppy extends Dog { weeks: number; constructor(k: string, b: string, w: number) { super(k, b); this.weeks = w; } }

class Vehicle { wheels: number; constructor(w: number) { this.wheels = w; } }

function check(): number {
  let p = new Puppy("dog", "lab", 8);
  let d = new Dog("dog", "lab");
  let a = new Animal("dog");
  let v = new Vehicle(4);

  // Puppy instance — should match all 3 ancestors.
  if (!(p instanceof Puppy))  { throw "#1: p-Puppy"; }
  if (!(p instanceof Dog))    { throw "#2: p-Dog"; }
  if (!(p instanceof Animal)) { throw "#3: p-Animal"; }
  if (p instanceof Vehicle)   { throw "#4: p-Vehicle false-positive"; }

  // Dog — matches Dog + Animal, not Puppy.
  if (!(d instanceof Dog))    { throw "#5: d-Dog"; }
  if (!(d instanceof Animal)) { throw "#6: d-Animal"; }
  if (d instanceof Puppy)     { throw "#7: d-Puppy false-positive"; }
  if (d instanceof Vehicle)   { throw "#8"; }

  // Animal — matches only Animal.
  if (!(a instanceof Animal)) { throw "#9"; }
  if (a instanceof Dog)       { throw "#10"; }
  if (a instanceof Puppy)     { throw "#11"; }

  // Cross-tree negative.
  if (v instanceof Animal)    { throw "#12"; }
  if (v instanceof Dog)       { throw "#13"; }
  if (!(v instanceof Vehicle)){ throw "#14"; }

  return 0;
}
console.log(check());
