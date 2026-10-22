// An async function whose returns are `T` for the parameter types it
// declares hands its handler a `T`; one whose return reads an `any`
// parameter hands the value as it is.

async function dbl(n: number): Promise<number> {
  return n * 2;
}

async function label(s: string): Promise<string> {
  return s + "!";
}

async function held(x: any): Promise<number> {
  return x;
}

dbl(4).then((v) => console.log("dbl", typeof v, v + 1));
label("a").then((v) => console.log("label", typeof v, v.length));
held("h").then((v) => console.log("held", typeof v, v));

async function main() {
  const v = await dbl(5);
  console.log("await dbl", v + 1);
  const s = await label("bc");
  console.log("await label", s.length);
}
main();
