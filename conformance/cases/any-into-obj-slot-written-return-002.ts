// A written object return type is checked in the copies the compiler
// makes of the function as in the function itself: a generic call's
// instance throws at the return. The copy an `any` array argument
// selects answers `any` when its return may depend on that argument
// (`JSON.parse` here), and that answer is read through the any lane
// unchecked. bun does not check the annotation, so this case is
// checked against its .expected.

function attempt(tag: string, f: () => void): void {
  try {
    f();
    console.log(tag, "ok");
  } catch (e) {
    console.log(tag, e instanceof TypeError, (e as Error).message);
  }
}

function sized(xs: number[], src: string): { x: number } {
  console.log("len", xs.length);
  return JSON.parse(src);
}
const typed: number[] = [1, 2];
const loose: any[] = JSON.parse("[1, 2, 3]");
attempt("typed ok", () => console.log(sized(typed, '{"x":1}').x));
attempt("typed bad", () => sized(typed, '{"x":"s"}'));
attempt("any arr ok", () => console.log(sized(loose, '{"x":2}').x));
attempt("any arr bad", () => sized(loose, '{"x":"s"}'));

function boxed<T>(t: T, src: string): { v: number } {
  console.log("t", t);
  return JSON.parse(src);
}
attempt("generic ok", () => console.log(boxed<string>("a", '{"v":3}').v));
attempt("generic bad", () => boxed<boolean>(true, '{"v":"s"}'));
attempt("generic inferred bad", () => boxed(7, "[]"));
