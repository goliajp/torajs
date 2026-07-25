// An async function with a concrete return annotation that runs off the
// end of its body still settles with `undefined` — ES §10.2.1.4 step 11
// makes the implicit tail completion carry no value, so the annotation
// says nothing about what that path answers. tr used to settle with the
// *type's* zero there (`0` for number, `""` for string), which an
// awaiter cannot tell apart from a real result.
//
// Each width spells undefined its own way: `number` needs the F64
// sentinel (and the wide slot to carry it), pointer-shaped types take
// their immortal cell. The last two cases pin the other side of that —
// a real NaN and a real zero must survive as themselves.

async function num(flag: boolean): Promise<number> {
  if (flag) return 1;
}

async function str(flag: boolean): Promise<string> {
  if (flag) return "x";
}

async function arr(flag: boolean): Promise<number[]> {
  if (flag) return [1];
}

const xs: number[] = [1];
async function miss(): Promise<number> {
  return xs.find((v) => v > 9);
}

async function realNaN(): Promise<number> {
  return 0 / 0;
}

async function realZero(): Promise<number> {
  return 0;
}

async function main() {
  console.log(await num(false));
  console.log(await num(true));
  console.log(typeof (await num(false)));
  console.log((await num(false)) === undefined);

  console.log(await str(false));
  console.log(await str(true));
  console.log(typeof (await str(false)));

  console.log(await arr(false));
  console.log(await arr(true));

  // a sentinel handed to Promise.resolve survives the round trip
  console.log(await miss());

  // ...and the values that merely look like it do not get mistaken for it
  console.log(await realNaN());
  console.log(Number.isNaN(await realNaN()));
  console.log(await realZero());
  console.log((await realZero()) === undefined);
}

main();
