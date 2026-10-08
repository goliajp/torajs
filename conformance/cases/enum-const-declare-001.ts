// const enum members read as constants; the object still exists for
// value uses; declare enum provides the type only
const enum Level {
  Low = 1,
  Mid = Low * 10,
  High = Mid * 10,
}

const enum Tag {
  A = "a",
  B = "b",
}

declare enum Ambient {
  One = 1,
  Two,
}

let l: Level = Level.Mid;
console.log(Level.Low, Level.Mid, Level.High, l, l === Level.Mid);
console.log(Level[1], Level[10], Level[100], Object.keys(Level));
console.log(Tag.A, Tag.B, Object.keys(Tag));

function pick(t: Tag): number {
  return t === Tag.A ? Level.Low : Level.High;
}
console.log(pick(Tag.A), pick(Tag.B));

let a: Ambient = 2;
a = 1;
console.log(a, typeof a);

function scale(x: number, by: Level = Level.Mid): number {
  return x * by;
}
console.log(scale(3), scale(3, Level.High), scale(3, Level.Low));
