// A catch binding shadows the function declaration of the same name
function pick(v: number): string {
  return "G" + typeof v;
}

function viaCatch(d: number[]): string {
  const u: any = d;
  u[0] = "s";
  try {
    throw (v: number) => "T";
  } catch (pick) {
    return d.map(pick).join(",");
  }
}
console.log(viaCatch([7, 8]));

function viaCatchUnused(d: number[]): string {
  const u: any = d;
  u[0] = "s";
  try {
    throw 1;
  } catch (pick) {
    return String(pick) + d.map((v: number) => "T" + v).join(",");
  }
}
console.log(viaCatchUnused([7, 8]));
console.log([1, 2].map(pick).join(","));
