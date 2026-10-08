// constant initializers that name other enums' members (dot and
// bracket), the enum's own members through its name, and `const`
// bindings without a type annotation: all are constants, so a member
// without an initializer after them counts up (tsc 6.0.3 accepts every
// declaration here). A name a nearer scope rebinds is not the const.
// The .expected file is tsc 6.0.3's emit run under node: bun 1.4.2
// does not fold const bindings and leaves the counted members undefined.
enum A {
  X = 3,
  S = "s",
}

enum B {
  Y = A.X,
  Z,
  W = A["X"] + 1,
  V,
  U = B.Y * 2,
  T = B["Z"],
  R,
}

const k = 7;
const ks = "pre";
const k2 = k + 1;
const kx = A.X * 10;
const kp = (2);

enum C {
  P = k,
  Q,
  PS = ks + "x",
  N = -k,
  M,
}

enum F {
  P = k2,
  Q,
  R = kx,
  S,
  T = kp,
  U,
}

enum L {
  P = A.S + "t",
  Q = `${ks}-${k}`,
  R = 0,
  S,
}

console.log(B.Y, B.Z, B.W, B.V, B.U, B.T, B.R);
console.log(Object.keys(B).join(","));
console.log(C.P, C.Q, C.PS, C.N, C.M, C[8], C[-6]);
console.log(F.P, F.Q, F.R, F.S, F.T, F.U);
console.log(L.P, L.Q, L.R, L.S);

function scoped(): string {
  const kk = 10;
  let inner = 0;
  {
    const kk = 20;
    enum G {
      P = kk,
      Q,
    }
    inner = G.Q;
  }
  enum H {
    P = kk,
    Q,
  }
  return inner + ":" + H.Q;
}
console.log(scoped());

function shadowParam(k: number): number {
  enum J {
    P = k,
    Q = 1,
  }
  return J.P * 100 + J.Q;
}
console.log(shadowParam(5), shadowParam(6));

function shadowLet(): number {
  let k = 40;
  k += 2;
  enum J {
    P = k,
    Q = 1,
  }
  return J.P;
}
console.log(shadowLet());

const kt: number = 5;
enum Annotated {
  P = kt,
  Q = 9,
}
console.log(Annotated.P, Annotated.Q, Annotated[5]);
