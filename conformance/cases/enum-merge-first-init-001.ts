// one declaration of a merged enum may omit its first member's
// initializer, and that declaration's members count up from 0 again
// (tsc 6.0.3 accepts all of these; a second omission is TS2432)
enum Tail {
  A = 1,
}
enum Tail {
  B,
  C,
}

enum Head {
  A,
  B,
}
enum Head {
  C = 5,
  D,
}

enum Empty {}
enum Empty {
  X,
}
enum Empty {
  Y = 4,
  Z,
}

console.log(Tail.A, Tail.B, Tail.C, Tail[0], Tail[1], Tail[2]);
console.log(Object.keys(Tail).join(","));
console.log(Head.A, Head.B, Head.C, Head.D, Head[5], Head[6]);
console.log(Object.keys(Head).join(","));
console.log(Empty.X, Empty.Y, Empty.Z, Empty[0], Empty[4], Empty[5]);
console.log(Object.keys(Empty).join(","));

function local(): string {
  enum L {
    P = 10,
  }
  enum L {
    Q,
    R,
  }
  return [L.P, L.Q, L.R, L[0], L[10]].join(",");
}
console.log(local());
