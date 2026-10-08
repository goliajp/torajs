// const enum members read as constants in switch cases, parameter
// types and default values; Object.keys / Object.values order with
// reverse mappings (integer keys first, ascending)
const enum Op {
  Add = 1,
  Sub,
  Mul = 10,
  Div,
}

const enum Unit {
  Px = "px",
  Em = "em",
}

function apply(op: Op, a: number, b: number): number {
  switch (op) {
    case Op.Add:
      return a + b;
    case Op.Sub:
      return a - b;
    case Op.Mul:
      return a * b;
    case Op.Div:
      return a / b;
  }
  return NaN;
}

function size(n: number, u: Unit = Unit.Px): string {
  switch (u) {
    case Unit.Px:
      return n + "px";
    case Unit.Em:
      return n / 16 + "em";
  }
  return "?";
}

const ops: Op[] = [Op.Add, Op.Sub, Op.Mul, Op.Div];
let total = 0;
for (const op of ops) {
  total += apply(op, 12, 4);
}
console.log(apply(Op.Add, 2, 3), apply(Op.Div, 9, 3), total);
console.log(size(32), size(32, Unit.Em));

enum Order {
  Z = 30,
  A = 2,
  M = "m",
  B = 1,
}
console.log(Object.keys(Order).join(","));
console.log(Object.values(Order).join(","));
console.log(Object.entries(Order).length);
console.log(Order[Order.Z], Order[Order.A], Order[Order.B], Order[30], Order["m"]);

function rev(o: Op): string {
  const names: string[] = [];
  names.push(o === Op.Add ? "add" : "other");
  return names.join("") + ":" + o;
}
console.log(rev(Op.Add), rev(Op.Div));

function dirOf(x: number): string {
  const enum Dir {
    Left = -1,
    None = 0,
    Right = 1,
  }
  let d: Dir = x < 0 ? Dir.Left : x > 0 ? Dir.Right : Dir.None;
  return d === Dir.Left ? "left" : d === Dir.Right ? "right" : "none";
}
console.log(dirOf(-5), dirOf(0), dirOf(7));

enum Level {
  Low = 0.5,
  High = 2,
}
const levels: Level[] = [Level.Low, Level.High, Level.Low];
let sum = 0;
for (const l of levels) {
  sum += l;
}
console.log(levels, sum);
