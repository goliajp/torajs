// an object an async function or method settles with, awaited into a
// binding and read field by field: a fractional field reaches an
// integer-looking array at its own width
class C {
  async o(): Promise<{ value: number }> {
    return { value: 0.75 };
  }
}
async function f(): Promise<{ value: number }> {
  return { value: 0.25 };
}
async function main(): Promise<void> {
  const a: number[] = [];
  const s = await new C().o();
  a.push(s.value);
  const t = await f();
  a.push(t.value);
  console.log(a.join(","));
}
main();
