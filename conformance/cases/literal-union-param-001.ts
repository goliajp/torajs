// A parameter, a return type and a field typed with a union of
// literal types (TS §3.2.10, §3.4): the value passes through calls,
// arrays and objects as its base type.

type Dir = "left" | "right" | "up" | "down";
type Code = 200 | 404 | 500;

function step(d: Dir, n: number): number {
  if (d === "left" || d === "down") return -n;
  return n;
}

function label(c: Code): string {
  switch (c) {
    case 200:
      return "ok";
    case 404:
      return "missing";
    default:
      return "error " + c;
  }
}

function flip(d: Dir): Dir {
  return d === "left" ? "right" : d === "right" ? "left" : d;
}

interface Move {
  dir: Dir;
  by: number;
}

const moves: Move[] = [
  { dir: "left", by: 2 },
  { dir: "up", by: 5 },
  { dir: "down", by: 1 },
  { dir: "right", by: 3 },
];
let pos = 0;
for (const mv of moves) {
  pos += step(mv.dir, mv.by);
  console.log(mv.dir, flip(mv.dir), pos);
}
const codes: Code[] = [200, 404, 500];
console.log(codes.map(label).join(", "));
console.log(codes.reduce((a, c) => a + c, 0));
