// `s[i]` / `s[k]` on a `string | undefined` receiver compiles, and an
// absent one raises the TypeError after the key has been evaluated.
//
// §13.3.2.1 evaluates the key expression (step 3) before the read's
// GetValue reaches §7.1.20 ToObject, so `s[side()]` prints SIDE first.
// The index checker stripped the wrapper only for arrays; a str
// receiver was refused, and once admitted its lane read through an
// absent value and answered `undefined`. The key now lowers first, is
// parked for the lane, and the str guard kernel runs before the read.
//
// Written out directly, never through a helper (r687).

function maybe(): string | undefined {
  return Math.random() < 2 ? "abc" : undefined;
}
function none(): string | undefined {
  return undefined;
}
function side(): number {
  console.log("SIDE");
  return 0;
}

function main(): void {
  const s: string | undefined = maybe();
  let i = 1;
  const k: string = "length";
  console.log(s[0], s[i], s[i + 5], s[k]);

  const gone: string | undefined = none();
  try {
    console.log(gone[0]);
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
  try {
    console.log(gone[side()]);
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
  try {
    console.log(gone[k]);
  } catch (e) {
    console.log((e as Error).constructor.name);
  }
}

main();
