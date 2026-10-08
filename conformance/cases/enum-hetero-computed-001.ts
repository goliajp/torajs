// heterogeneous members, computed members evaluated once at runtime,
// auto-increment continuing after a computed member, non-index
// numeric values and their reverse entries
let calls = 0;
function next(): number {
  calls += 1;
  return calls * 10;
}

enum Mixed {
  No = 0,
  Yes = "YES",
  Maybe = 7,
  Later,
}

enum Computed {
  A = "abc".length,
  B,
  C = next(),
  D = C + 1,
  E = 100,
  F = E * 2,
}

enum Weird {
  Neg = -1,
  Half = 1.5,
  Two = 2,
  Big = 4294967295,
  Zero = 0,
}

console.log(Mixed.No, Mixed.Yes, Mixed.Maybe, Mixed.Later);
console.log(Object.keys(Mixed), Mixed[7], Mixed[8]);
console.log(Mixed);

console.log(Computed.A, Computed.B, Computed.C, Computed.D, Computed.E, Computed.F);
console.log(calls);
console.log(Computed[3], Computed[4], Computed[10], Computed[11], Computed[200]);
console.log(Object.keys(Computed));
console.log(Computed);

console.log(Weird.Neg, Weird.Half, Weird.Two, Weird.Big, Weird.Zero);
console.log(Weird[-1], Weird[1.5], Weird[2], Weird[4294967295], Weird[0]);
console.log(Object.keys(Weird));
console.log(Weird);
