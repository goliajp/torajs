// an enum's type covers every merged declaration (a string member
// joining numeric ones makes it heterogeneous), and two local enums of
// one name in different functions keep their own types
enum S {
  A = 1,
}
enum S {
  B = "x",
}
const s: S = S.B;
const n: S = S.A;
console.log(s, n, typeof s, typeof n);

function numeric(): number {
  enum Local {
    A = 1,
    B,
  }
  const v: Local = Local.B;
  return v * 10;
}

function text(): string {
  enum Local {
    A = "z",
    B = "y",
  }
  const v: Local = Local.A;
  return v + Local.B + "!";
}

function both(): string {
  enum Local {
    A = 3,
  }
  enum Local {
    B = "w",
  }
  const a: Local = Local.A;
  const b: Local = Local.B;
  return a + "/" + b;
}
console.log(numeric(), text(), both());

function pick(flag: boolean): string {
  enum K {
    On = "on",
    Off = "off",
  }
  const k: K = flag ? K.On : K.Off;
  return k.toUpperCase();
}
console.log(pick(true), pick(false));
