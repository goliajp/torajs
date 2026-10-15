// `(o?.m).call(o, x)`: the parentheses end the optional chain
// (§13.3.9), so a nullish `o` yields `undefined` for the parenthesized
// part and `.call` on it throws a TypeError. Without them,
// `o?.m.call(o, x)` short-circuits as a whole to `undefined`.

type T = { tag: string; m(x: number): string };
function make(n: number): T | null {
  return n > 0
    ? {
        tag: "t" + n,
        m(x: number) {
          return this.tag + ":" + x;
        },
      }
    : null;
}

function paren(o: T | null): string {
  try {
    return String((o?.m).call(o, 1));
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}
function bare(o: T | null): string {
  return String(o?.m.call(o, 2));
}
function parenTail(o: T | null): string {
  try {
    return String((o?.m.call)(o, 3));
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}

const n = process.argv.length > 99 ? 0 : 1;
const a = make(n);
const z = make(n - 1);
console.log(paren(a), paren(z));
console.log(bare(a), bare(z));
console.log(parenTail(z));
