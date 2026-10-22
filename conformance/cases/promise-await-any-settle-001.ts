// A `new Promise<number>` whose executor resolves with an `any` that
// holds a string settles that string (§27.2.1.3.2 resolves with the
// value as it is); `await` reads it back as the string.

async function main() {
  const x: any = "s";
  const p = new Promise<number>((r) => r(x));
  const n = await p;
  console.log(typeof n, n);
  const y: any = 7;
  const m = await new Promise<number>((r) => r(y));
  console.log(typeof m, m + 1);
}
main();
