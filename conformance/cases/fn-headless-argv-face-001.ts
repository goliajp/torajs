// RFC 20260816-headless-argv-face — a head-less top-level fn whose
// call sites pass DIFFERENT argument counts reads the true values
// through the runtime argv channel, not just its declared params.
// Before the face, `arguments.length` answered the real count (the
// H1 hidden argc slot) while every beyond-declared `arguments[i]`
// answered undefined — a silent-wrong the static-argv face could not
// cover because it needs one uniform count across all sites.

function join(a: number) {
  let s = "";
  for (let i = 0; i < arguments.length; i++) {
    s += String(arguments[i]) + ",";
  }
  return s;
}

// Two differently-sized sites — this is what kicks the fn off the
// static face.
console.log(join(1, 2, 3));
console.log(join(9));
console.log(join(1, "two", true, null));

// Mixed value shapes: a call result (owned temp), an object, an
// array, a string literal.
function make(): string {
  return "made";
}
console.log(join(1, make(), { k: 2 }, [3, 4], "lit"));

// The declared param still reads normally alongside the array.
function both(a: number) {
  return "a=" + a + " n=" + arguments.length + " last=" + String(arguments[arguments.length - 1]);
}
console.log(both(7, 8, 9));
console.log(both(7));

// A defaulted param: the pad `apply_default_args` appends is the
// callee's own value, not an argument the program passed, so it must
// not widen `arguments`.
function dflt(a: number = 5) {
  return "a=" + a + " n=" + arguments.length + " [0]=" + String(arguments[0]);
}
console.log(dflt());
console.log(dflt(1, 2));

// Element writes ride the materialized array (module code is strict,
// so arguments is unmapped — writing arguments[0] must NOT touch `a`).
function write(a: number) {
  arguments[0] = 99;
  return "a=" + a + " [0]=" + String(arguments[0]) + " [1]=" + String(arguments[1]);
}
console.log(write(1, 2));

// Spreading the object out.
function spread(a: number) {
  return [...arguments].join("-");
}
console.log(spread(1, 2, 3));
console.log(spread(4));

// Recursion: each frame packs its own buffer.
function rec(n: number) {
  if (n <= 0) {
    return "";
  }
  let s = "";
  for (let i = 0; i < arguments.length; i++) {
    s += String(arguments[i]) + ";";
  }
  return s + rec(n - 1, "x", "y");
}
console.log(rec(2, "a"));

// Beyond-declared reads past the actual count answer undefined, and
// an explicitly passed undefined still counts (ES §10.4.4 builds the
// object from the argument list, not from the bound parameters).
function edge(a: number) {
  return arguments.length + ":" + String(arguments[1]) + ":" + String(arguments[5]);
}
console.log(edge(1, "s"));
console.log(edge(1, undefined));
console.log(edge(1, null));
