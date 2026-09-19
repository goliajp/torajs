// A computed key is evaluated in the scope its class is written in
// (§15.7.14), so a key that reads a sibling class declared in the same
// function sees that class — whichever lane the sibling takes and
// whatever name the hoist gives it.
function capturing(k: number): any {
  class Holder { static tag = "H" + k; }
  class KeyedC { static [Holder.tag + "_key"] = k; }
  return KeyedC;
}
function unit(): any {
  class Holder { static tag = "U"; }
  class KeyedU { static [Holder.tag + "_key"] = 1; m(): number { return 2; } }
  return KeyedU;
}
// the two `Holder`s collide and the hoist renames them
function collideA(): any {
  class Holder { static tag = "CA"; }
  class KeyedA { static [Holder.tag] = 1; }
  return KeyedA;
}
function collideB(): any {
  class Holder { static tag = "CB"; }
  class KeyedB { static [Holder.tag] = 2; }
  return KeyedB;
}
function local(k: number): any {
  const t: string = "L" + k;
  class KeyedL { static [t] = k; }
  return KeyedL;
}
function nested(k: number): any {
  class Outer {
    static tag = "O" + k;
    static Inner = class { static [Outer.tag + "_in"] = k; };
  }
  return Outer.Inner;
}
console.log(Object.keys(capturing(2)).join(","), Object.keys(capturing(3)).join(","));
console.log(Object.keys(unit()).join(","), new (unit())().m());
console.log(Object.keys(collideA()).join(","), Object.keys(collideB()).join(","));
console.log(Object.keys(local(4)).join(","), Object.keys(nested(5)).join(","));
