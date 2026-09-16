// §7.3.31 PrivateGet / §7.3.32 PrivateSet — the element's kind decides:
// an accessor without a getter (read) and a method (write) throw
// TypeError; a Proxy never carries its target's private elements.
class C {
  set #f(v: number) { console.log("set", v); }
  get #g() { return 7; }
  #m() { return 1; }
  #x = 5;
  readSetterOnly() { return this.#f; }
  readSetterOnlyAny() { const o: any = this; return o.#f; }
  writeGetterOnly() { this.#g = 3; }
  writeMethodAny() { (this as any).#m = 3; }
  readField(o: any) { return o.#x; }
  writeField(o: any) { o.#x = 9; return o.#x; }
  hasField(o: any) { return #x in o; }
  callMethod(o: any) { return o.#m(); }
  okPaths() { this.#f = 4; return [this.#g, this.#m(), this.#x]; }
}
const c = new C();
const p = new Proxy(c, {});
const probes: [string, () => unknown][] = [
  ["readSetterOnly", () => c.readSetterOnly()],
  ["readSetterOnlyAny", () => c.readSetterOnlyAny()],
  ["writeGetterOnly", () => c.writeGetterOnly()],
  ["writeMethodAny", () => c.writeMethodAny()],
  ["proxy read field", () => c.readField(p)],
  ["proxy write field", () => c.writeField(p)],
  ["proxy has field", () => c.hasField(p)],
  ["proxy call method", () => c.callMethod(p)],
  ["ok paths", () => c.okPaths()],
  ["plain read field", () => c.readField(c)],
  ["foreign call method", () => c.callMethod({})],
];
for (const [name, f] of probes) {
  try {
    console.log(name, "->", f());
  } catch (e) {
    console.log(name, "throws", (e as any) instanceof TypeError);
  }
}
