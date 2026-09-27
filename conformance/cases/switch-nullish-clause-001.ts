// `case null` / `case undefined` and clauses of another type against
// scrutinees of every representation. A string scrutinee against a
// number clause called the string compare on the number and crashed;
// `false` matched `case null`; an argument that is `undefined` never
// matched `case undefined`; and an out-of-range read of a number array
// never matched it either.
function kind(value: any): string {
  switch (value) {
    case 0: return "zero";
    case null: return "null";
    case undefined: return "undef";
    case "0": return "str0";
    case false: return "false";
    default: return "other";
  }
}

function implicit(value) {
  switch (value) {
    case 0: return "zero";
    case null: return "null";
    case undefined: return "undef";
    default: return "other";
  }
}

function at(a: number[], i: number): string {
  const v = a[i];
  switch (v) {
    case undefined: return "hole";
    case 1.5: return "x";
    default: return "d";
  }
}

console.log(kind(0), kind(null), kind(undefined), kind("0"), kind(false), kind(true));
console.log(implicit(0), implicit(null), implicit(void 0), implicit(false), implicit("0"));
console.log(at([1.5, 2.5], 0), at([1.5, 2.5], 1), at([1.5, 2.5], 7));
