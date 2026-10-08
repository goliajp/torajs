// numeric enum: auto-increment, explicit values, constant expressions
// over earlier members, reverse mapping, the enum name as a type
enum Color {
  Red,
  Green,
  Blue,
}

enum Flags {
  None = 0,
  A = 1 << 0,
  B = 1 << 1,
  C = 1 << 2,
  AB = A | B,
  All = A | B | C,
  Big = 7 * 6 - 2,
  Neg = -3,
  Shifted = Big >> 1,
  Masked = All & ~B,
}

console.log(Color.Red, Color.Green, Color.Blue);
console.log(Color[0], Color[1], Color[2], Color[Color.Blue]);
console.log(Flags.None, Flags.A, Flags.B, Flags.C, Flags.AB, Flags.All);
console.log(Flags.Big, Flags.Neg, Flags.Shifted, Flags.Masked);
console.log(Flags[3], Flags[7], Flags[40], Flags[-3]);
console.log(Object.keys(Color));
console.log(Object.keys(Flags));
console.log(typeof Color, typeof Color.Red, typeof Color[0]);
console.log(JSON.stringify(Color));
console.log(Color);

let c: Color = Color.Red;
c = Color.Green;
console.log(c, c === Color.Green, c === Color.Red, Color[c]);

function describe(x: Color): string {
  switch (x) {
    case Color.Red:
      return "warm";
    case Color.Green:
      return "fresh";
    case Color.Blue:
      return "cool";
  }
  return "none";
}
console.log(describe(Color.Red), describe(Color.Green), describe(c), describe(Color.Blue));

function hasFlag(v: number, f: Flags): boolean {
  return (v & f) !== 0;
}
console.log(hasFlag(Flags.AB, Flags.A), hasFlag(Flags.AB, Flags.C), hasFlag(Flags.All, Flags.C));

for (const k in Color) {
  console.log(k, Color[k]);
}
const names: string[] = [];
for (const k of Object.keys(Flags)) {
  names.push(k);
}
console.log(names.length, names.join(","));
