// §10.1.6.3 step 4: a locked declared field takes a redefine whose
// value is SameValue with the current one, compared as values: an
// integer against a field held as a double, two equal strings built
// apart. -0 against +0 and a different value still refuse.
class C { x: number; y: string; constructor() { this.x = 0.5; this.y = "a"; } }
function lockAndRedefine(label: string, key: "x" | "y", first: any, again: any): void {
  const c = new C();
  Object.defineProperty(c, key, { value: first, writable: false, configurable: false });
  try {
    Object.defineProperty(c, key, { value: again });
    console.log(label, "same-ok", c[key]);
  } catch (err) {
    console.log(label, "threw", (err as any) instanceof TypeError, c[key]);
  }
}
const n = Number(process.argv.length);
lockAndRedefine("int", "x", 5, 5);
lockAndRedefine("double", "x", 2.5, 2.5);
lockAndRedefine("int-vs-double", "x", n * 3, 3 * n + 0.0);
lockAndRedefine("zero", "x", 0, -0);
lockAndRedefine("other", "x", 5, 6);
lockAndRedefine("str", "y", "ab" + n, "a" + "b" + n);
lockAndRedefine("str-other", "y", "ab", "ba");
