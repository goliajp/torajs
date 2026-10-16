// §13.3.9: a parenthesized optional chain ends the chain, so
// `(o?.[k])()` on a nullish `o` calls `undefined` and throws a
// TypeError; on a live `o` it calls the method with `o` as `this`.
const k = "m";
const none: { v: number; m(): number } | undefined = undefined;
try {
  (none?.[k])();
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name);
}
let n: any = null;
try {
  (n?.[k])();
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name);
}
const o: { v: number; m(): number } | undefined = {
  v: 4,
  m() {
    return this.v;
  },
};
console.log((o?.[k])());
const r: Record<string, () => string> | null = null;
try {
  (r?.["x"])();
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name);
}
n = { m: () => 5 };
console.log((n?.[k])());
// the key of a short-circuited link never evaluates; the arguments do
let keys = 0;
let argsRun = 0;
const key = () => {
  keys++;
  return "m";
};
try {
  (none?.[key()])(argsRun++);
} catch (e) {
  console.log((e as Error).constructor.name, keys, argsRun);
}
console.log(none?.[key()](argsRun++), keys, argsRun);
console.log(o?.[key()](), keys);
