// A module's top-level declarations live in the module environment,
// not on the global object (§9.1.1.5): `globalThis.X` still reads the
// builtin beside a top-level `const` / `let` / `class` named `X`.

const process = 1;
console.log(process, globalThis.process.argv.length > 0);
let parseInt = (s: string): number => 7;
console.log(parseInt("3"), globalThis.parseInt("3"));
let Math = { floor: (x: number): number => -1 };
console.log(Math.floor(2.5), globalThis.Math.floor(2.5));
class JSON {
  static stringify(x: number): string {
    return "mine";
  }
}
console.log(JSON.stringify(1), globalThis.JSON.stringify([1]));
var escape = (s: string): string => "u:" + s;
console.log(escape("a b"), globalThis.escape("a b"));
