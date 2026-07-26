// The `return` counterpart of the container walk: a function whose
// declared return type is a function type contextually types an arrow
// it returns. Without it the arrow's parameter took the same
// contextless `string` default a field or an element used to, so
// `make()(3)` answered 3.

function make(): (n: number) => number {
  return (n) => n + 1;
}
console.log("ret-arrow", make()(3));

function pick(b: boolean): (n: number) => number {
  if (b) {
    return (n) => n + 1;
  }
  return (n) => n * 2;
}
console.log("ret-branch", pick(true)(3), pick(false)(3));

function makeStr(): (s: string) => string {
  return (s) => s + "!";
}
console.log("ret-string", makeStr()("hi"));

function makeTwo(): (a: number, b: number) => number {
  return (a, b) => a * 100 + b;
}
console.log("ret-two-params", makeTwo()(3, 7));

function makeBool(): (a: boolean) => boolean {
  return (a) => !a;
}
console.log("ret-boolean", makeBool()(true));

function makeVoid(): (n: number) => void {
  return (n) => {
    console.log("  ret-void inside", n);
  };
}
makeVoid()(3);

// Shapes that already worked and must keep working: returning a named
// function, a non-function return type, and an arrow bound to a name
// before being returned.
function inc(n: number): number {
  return n + 1;
}
function makeNamed(): (n: number) => number {
  return inc;
}
console.log("ret-named-fn", makeNamed()(3));

function makeBound(): (n: number) => number {
  const f = (n: number): number => n + 1;
  return f;
}
console.log("ret-bound", makeBound()(3));

function plain(): number {
  return 7;
}
console.log("ret-non-fn", plain());
