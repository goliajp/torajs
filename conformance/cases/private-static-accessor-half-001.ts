// §7.3.31 step 6 / §7.3.32 step 5.c on STATIC private accessors: a
// read with no getter and a write with no setter throw TypeError.
class C {
  static set #w(v: number) { console.log("set", v); }
  static get #r() { return 7; }
  static #both = 0;
  static get #rw() { return C.#both; }
  static set #rw(v: number) { C.#both = v; }
  static readWriteOnly() { return this.#w; }
  static writeReadOnly() { this.#r = 3; }
  static okPaths() {
    this.#w = 1;
    this.#rw = 5;
    return [this.#r, this.#rw];
  }
}
const probes: [string, () => unknown][] = [
  ["read setter-only", () => C.readWriteOnly()],
  ["write getter-only", () => C.writeReadOnly()],
  ["ok paths", () => C.okPaths()],
];
for (const [name, f] of probes) {
  try {
    console.log(name, "->", f());
  } catch (e) {
    console.log(name, "throws", (e as any) instanceof TypeError);
  }
}
