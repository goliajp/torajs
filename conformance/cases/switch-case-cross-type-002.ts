// A case value of a different type from the scrutinee is never
// strictly equal to it, but its expression is still evaluated. `null`
// against `case 0`, `false` against `case 0` and `0` against
// `case false` compared their raw bits, which are equal, and selected
// the clause.
let log = "";
function note(s: string, v: any): any {
  log += s;
  return v;
}

function onNull(): string {
  let out = "";
  switch (null) {
    case note("a", 0): out += "zero";
    default: out += "d";
    case note("b", false): out += "f";
  }
  return out;
}

function onFalse(b: boolean): string {
  switch (b) {
    case 0 as any: return "zero";
    case false: return "false";
    default: return "other";
  }
}

function onZero(n: number): string {
  switch (n) {
    case false as any: return "false";
    case null as any: return "null";
    case 0: return "zero";
    default: return "other";
  }
}

console.log(onNull(), log);
console.log(onFalse(false), onZero(0));
