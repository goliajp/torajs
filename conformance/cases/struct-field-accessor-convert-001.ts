// A getter installed over a declared field answers through the field's
// type: a value of that type (or `undefined`, when the annotation admits
// it) crosses as it is; any other value throws a TypeError at the read.
// bun keeps whatever the getter returned (an annotation does nothing at
// run time), so this case is checked against its .expected: the throw is
// a deliberate departure, an annotated slot holds what its annotation
// says, never a silently converted value.
class Cell {
  v: number = 1;
  u?: number;
  s: string = "a";
  t: string | undefined = "b";
  b: boolean = false;
  n: bigint = 1n;
  f: () => number = () => 10;
}
function attempt(label: string, read: () => unknown): void {
  try {
    console.log(label, read());
  } catch (e) {
    console.log(label, e instanceof TypeError, (e as Error).message);
  }
}
function getter(c: Cell, key: string, value: unknown): void {
  Object.defineProperty(c, key, { get: () => value, configurable: true });
}
const c = new Cell();
getter(c, "v", "5");
attempt("number from string", () => c.v + 1);
getter(c, "v", { valueOf: () => 3 });
attempt("number from object", () => c.v);
getter(c, "v", undefined);
attempt("number from undefined", () => c.v);
getter(c, "u", undefined);
attempt("optional number from undefined", () => c.u);
getter(c, "u", 3);
attempt("optional number from number", () => c.u);
getter(c, "v", 2.5);
attempt("number from number", () => c.v * 2);
getter(c, "s", 4);
attempt("string from number", () => c.s);
getter(c, "t", undefined);
attempt("optional string from undefined", () => c.t);
getter(c, "s", undefined);
attempt("string from undefined", () => c.s);
getter(c, "s", "ok");
attempt("string from string", () => c.s + "!");
getter(c, "b", 1);
attempt("boolean from number", () => c.b);
getter(c, "b", true);
attempt("boolean from boolean", () => c.b);
getter(c, "n", 5);
attempt("bigint from number", () => c.n);
getter(c, "n", 5n);
attempt("bigint from bigint", () => c.n + 1n);
getter(c, "f", () => 20);
attempt("closure from another closure", () => c.f());
getter(c, "f", 3);
attempt("closure from number", () => c.f());
