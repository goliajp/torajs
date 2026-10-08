// const enum members read as constants and the enum has no object at
// runtime (tsc rejects reading one as a value, TS2475 / TS2476);
// `declare const enum` members are inlined the same way; declare enum
// provides the type only
// The .expected file is tsc 6.0.3's emit run under node: bun 1.4.2
// does not inline `declare const enum` and throws a ReferenceError.
const enum Level {
  Low = 1,
  Mid = Low * 10,
  High = Mid * 10,
}

const enum Tag {
  A = "a",
  B = "b",
}

declare const enum Flags {
  None = 0,
  Read = 1 << 0,
  Write = 1 << 1,
  All = Read | Write,
}

declare enum Ambient {
  One = 1,
  Two,
}

let l: Level = Level.Mid;
console.log(Level.Low, Level.Mid, Level["High"], l, l === Level.Mid);
console.log(Tag.A, Tag["B"]);
console.log(Flags.None, Flags.Read, Flags.Write, Flags.All);

function pick(t: Tag): number {
  return t === Tag.A ? Level.Low : Level.High;
}
console.log(pick(Tag.A), pick(Tag.B));

function can(f: Flags, want: Flags): boolean {
  return (f & want) === want;
}
console.log(can(Flags.All, Flags.Write), can(Flags.Read, Flags.Write));

let a: Ambient = 2;
a = 1;
console.log(a, typeof a);

function scale(x: number, by: Level = Level.Mid): number {
  return x * by;
}
console.log(scale(3), scale(3, Level.High), scale(3, Level.Low));
