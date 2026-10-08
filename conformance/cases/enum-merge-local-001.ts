// declaration merging at the top level, enums local to functions and
// classes, two local enums sharing a name, and a member write that
// the folded reads do not see (tsc and bun inline the declaration)
enum Status {
  Active,
  Inactive,
}
enum Status {
  Pending = 5,
  Archived,
}

console.log(Status.Active, Status.Inactive, Status.Pending, Status.Archived);
console.log(Status[0], Status[5], Status[6], Object.keys(Status));

function area(kind: number): number {
  enum Shape {
    Square = 2,
    Cube = 3,
  }
  return kind === Shape.Square ? 4 * 4 : 4 * 4 * 4;
}
console.log(area(2), area(3));

function first(): string {
  enum Local {
    X = "first-x",
    Y = "first-y",
  }
  return Local.X + Local.Y + Object.keys(Local).length;
}
function second(): string {
  enum Local {
    X = "second-x",
    Z = 9,
  }
  return Local.X + Local[9] + Local.Z;
}
console.log(first(), second());

class Box {
  size(): number {
    enum Dim {
      W = 10,
      H = 20,
    }
    return Dim.W * Dim.H;
  }
}
console.log(new Box().size());

enum Mutable {
  A,
  B,
}
Mutable.A = 42;
console.log(Mutable.A, Mutable["A"], Mutable.B);
console.log(Mutable);
Mutable["B"] = 7;
console.log(Mutable.B, Object.keys(Mutable).length);

const arrow = (): number => {
  enum Inner {
    P = 3,
    Q = P * P,
  }
  return Inner.Q;
};
console.log(arrow());
