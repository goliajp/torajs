// An own-property walk that raises part way hands nothing back: the
// kernel settles the half-built array itself, since the throw edge
// releases nothing it was handed. Every exit answers the error it
// raised, a second call after the catch still works, and the normal
// path beside it is unchanged. Proxy traps (ownKeys, get,
// getOwnPropertyDescriptor), a struct getter, and ToObject on null.
const keysTrap = new Proxy({ pad: 1 }, { ownKeys() { throw new Error("keys"); } });
const getTrap = new Proxy({ a: [1, 2], b: 2 }, {
  get(t, k) { if (k === "b") { throw new Error("get"); } return t[k]; },
});
const descTrap = new Proxy({ a: 1, b: 2 }, {
  getOwnPropertyDescriptor(t, k) {
    if (k === "b") { throw new Error("desc"); }
    return Reflect.getOwnPropertyDescriptor(t, k);
  },
});
const struct = { pad: [1, 2, 3], get a(): number { throw new Error("getter"); }, z: "z" };
const nothing: any = null;
const probes: Array<() => unknown> = [
  () => Object.keys(keysTrap),
  () => Object.getOwnPropertyNames(keysTrap),
  () => Object.getOwnPropertySymbols(keysTrap),
  () => Reflect.ownKeys(keysTrap),
  () => Object.values(getTrap),
  () => Object.entries(getTrap),
  () => Object.keys(descTrap),
  () => Object.values(struct),
  () => Object.entries(struct),
  () => Object.values(nothing),
  () => Object.entries(nothing),
];
for (let round = 0; round < 2; round = round + 1) {
  const out: string[] = [];
  for (const p of probes) {
    try { p(); out.push("no throw"); } catch (e) { out.push(e.constructor.name); }
  }
  console.log(out.join(" "));
}
const plain = new Proxy({ a: 1, b: [2] }, {});
console.log(JSON.stringify(Object.keys(plain)), JSON.stringify(Object.values(plain)), JSON.stringify(Object.entries(plain)));
console.log(JSON.stringify(Reflect.ownKeys(plain)), Object.getOwnPropertySymbols(plain).length);
