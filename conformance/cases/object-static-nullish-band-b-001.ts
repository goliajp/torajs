// Band B at a CALL position — the first one. §20.1.2.1 step 1 and
// §20.1.2.23 step 1 are both ToObject(arg), and §7.1.18 answers null
// and undefined with a TypeError. The argument type is known while
// compiling, so the throw is decided there; the program still
// compiles and still throws, which is the thing neither neighbour
// does (bun must run ToObject to find out, tsc reports and hands
// back no executable).
function t(tag: string, f: () => unknown): void {
  try {
    console.log(tag, "ok", f());
  } catch (e: any) {
    console.log(tag, "throw", e.constructor.name);
  }
}

// §13.3.6.1 evaluates the arguments BEFORE the call, so their side
// effects happen even though the call was decided to throw. This is
// the property that makes emitting the throw the same program
// rather than merely the same outcome.
let seen = 0;
function side(): { a: number } {
  seen = 1;
  return { a: 1 };
}
t("assign-null", () => Object.assign(null, side()));
console.log("side ran:", seen);

t("assign-undef", () => Object.assign(undefined, { a: 1 }));
t("values-null", () => Object.values(null));
t("values-undef", () => Object.values(undefined));
console.log("after");
