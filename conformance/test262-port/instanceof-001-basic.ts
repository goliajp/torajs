// Adapted from test262 language/expressions/instanceof/* — basic
// class identity. tr is statically typed so `x instanceof C` is
// resolved at compile time: the operand's static SSA struct-id is
// compared with the alias entry for the named class. Direct-class
// identity only at this milestone — subclass→parent walk lands with
// M-OO.3.
class Animal { name: string; constructor(n: string) { this.name = n; } }
class Vehicle { wheels: number; constructor(w: number) { this.wheels = w; } }

function check(): number {
  let a = new Animal("dog");
  let v = new Vehicle(4);

  // Direct positive matches.
  if (!(a instanceof Animal)) { throw "#1: animal-is-Animal"; }
  if (!(v instanceof Vehicle)) { throw "#2: vehicle-is-Vehicle"; }

  // Cross-type negative.
  if (a instanceof Vehicle) { throw "#3: animal-is-not-Vehicle"; }
  if (v instanceof Animal) { throw "#4: vehicle-is-not-Animal"; }

  // Use as a guard expression.
  let n = a instanceof Animal ? 1 : 0;
  if (n !== 1) { throw "#5: ternary-guard"; }

  // Compose with logical operators.
  let both = (a instanceof Animal) && (v instanceof Vehicle);
  if (!both) { throw "#6: and-of-instanceofs"; }
  let either = (a instanceof Vehicle) || (v instanceof Animal);
  if (either) { throw "#7: or-of-mismatches"; }

  return 0;
}
console.log(check());
