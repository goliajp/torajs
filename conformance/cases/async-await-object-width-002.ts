// the same through an `any` receiver: the step an async `next` settles
// with keeps its fractional `value` (it was cut to an integer)
class Halves {
  n: number = 0;
  async next(): Promise<{ value: number; done: boolean }> {
    this.n = this.n + 1;
    return { value: this.n + 0.5, done: false };
  }
}
async function main(): Promise<void> {
  const it: any = new Halves();
  const t = await it.next();
  const a: number[] = [];
  a.push(t.value);
  const u: any = await it.next();
  const b: number[] = [];
  b.push(u.value);
  console.log(a.join(","), b.join(","));
}
main();
