// enum member types (`E.M` in a type position), member reads before
// the declaration inside a function, loop-local enums with computed
// members, keyword and quoted member names, constant templates and
// concatenation, numeric edge values
function early(): number {
  return Late.B;
}

enum Late {
  A,
  B,
}
console.log(early());

let r: Late.B = Late.B;
const first: Late.A = Late.A;
console.log(r, first, r + first);

enum Text {
  Hello = "hello",
  World = `world`,
  Both = `${Hello} ${World}`,
  Count = "n" + 2,
}
let t: Text.Hello = Text.Hello;
console.log(t, Text.World, Text.Both, Text.Count, Object.keys(Text).length);

function shadow(Late: { A: number }): number {
  return Late.A;
}
console.log(shadow({ A: 99 }), Late.A);

for (let i = 0; i < 3; i++) {
  enum Inner {
    P = i * 10,
    Q = 5,
  }
  console.log(Inner.P, Inner.Q, Inner[i * 10], Inner[5]);
}

enum Words {
  default = 1,
  class = 2,
  "with space" = 3,
}
console.log(Words.default, Words.class, Words["with space"], Words[3]);

enum Edge {
  Max = 2 ** 31,
  Frac = 1.5,
  Neg = -1,
  Inf = 1 / 0,
}
console.log(Edge.Max, Edge.Frac, Edge.Neg, Edge.Inf);
console.log(Object.keys(Edge).join(","));
console.log(Edge[1.5], Edge[-1], Edge[2147483648], Edge[Infinity]);
