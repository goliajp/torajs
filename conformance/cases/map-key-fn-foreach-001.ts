// A function stored as a Map key comes back as the second argument of
// the Map's forEach callback; a call through that parameter's declared
// signature reads the result the stored function produced.

function run(h: number) {
  const reg = new Map<() => number, string>();
  reg.set(() => h / 2, "half");
  reg.forEach((v: string, k: () => number) => console.log(v, k() + 1));
  for (const k of reg.keys()) console.log(k() * 2);
  for (const [k, v] of reg) console.log(v, k() - 1);
  const ints = new Map<() => number, number>();
  ints.set(() => h * 3, 0);
  ints.forEach((v: number, k: () => number) => console.log(v + k()));
}
run(7);
