// A `default` clause written between cases falls through into the case
// written after it, and the case before it falls into the default.
// §14.12.4 still tests every case before settling on the default.
// The clause used to be moved to the end, so control fell from the
// case before it past the default, and out of the default past the
// cases after it.
function mid(x: number): string {
  let out = "";
  switch (x) {
    case 1: out += "a";
    default: out += "d";
    case 2: out += "b"; break;
    case 3: out += "c";
  }
  return out;
}

function first(x: number): string {
  let out = "";
  switch (x) {
    default: out += "d";
    case 1: out += "a";
    case 2: out += "b";
  }
  return out;
}

function onlyDefault(x: number): string {
  switch (x) {
    default: return "d" + x;
  }
}

console.log(mid(1), mid(2), mid(3), mid(9));
console.log(first(1), first(2), first(9));
console.log(onlyDefault(4));
