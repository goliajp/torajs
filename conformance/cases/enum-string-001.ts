// string enum: no reverse mapping, literal concatenation, string and
// keyword member names, the enum name as a type
enum Direction {
  Up = "UP",
  Down = "DOWN",
  Left = "LE" + "FT",
  Right = "RIGHT",
}

enum Odd {
  "with-dash" = "dash",
  default = "dflt",
  class = "cls",
  "123abc" = "mixed",
}

console.log(Direction.Up, Direction.Down, Direction.Left, Direction.Right);
console.log(Object.keys(Direction));
console.log(JSON.stringify(Direction));
console.log(Direction);
console.log(Direction["Up"], Direction["UP"]);

let d: Direction = Direction.Up;
d = Direction.Left;
console.log(d, d === Direction.Left, d === Direction.Up, d.length, d.toLowerCase());

function turn(x: Direction): Direction {
  switch (x) {
    case Direction.Up:
      return Direction.Right;
    case Direction.Right:
      return Direction.Down;
    case Direction.Down:
      return Direction.Left;
    default:
      return Direction.Up;
  }
}
console.log(turn(Direction.Up), turn(turn(Direction.Up)), turn(d), turn(turn(turn(turn(Direction.Up)))));

console.log(Odd["with-dash"], Odd.default, Odd.class, Odd["123abc"]);
console.log(Object.keys(Odd));
for (const k in Odd) {
  console.log(k, "=>", Odd[k]);
}
const vals: string[] = [];
for (const k of Object.keys(Direction)) {
  vals.push(Direction[k]);
}
console.log(vals.join("|"));
