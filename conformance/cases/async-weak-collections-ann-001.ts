async function main() {
  const t = { a: 1 };
  const w = new WeakRef(t);
  const m = new WeakMap<object, number>();
  const s = new WeakSet<object>();
  m.set(t, 2); s.add(t);
  await null;
  console.log((w.deref() as any).a, m.get(t), s.has(t));
}
main();
