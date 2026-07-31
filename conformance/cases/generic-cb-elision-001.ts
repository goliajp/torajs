// Generic-call callback parameter elision — a callback accepting
// fewer parameters than the pattern provides is TS-compatible; the
// invoke still passes the pattern's argument count and the callee
// reads its prefix.
function apply2<T>(f: (a: T, b: T) => T, x: T, y: T): T {
  return f(x, y);
}
// full arity
console.log(apply2((a: number, b: number) => a + b, 3, 4));
// one-param callback (elision)
console.log(apply2((a: number) => a * 10, 3, 4));
// zero-param callback
console.log(apply2(() => 7, 3, 4));

// elision through a HOF whose typevar binds via the data argument
function mapOne<T, U>(v: T, f: (x: T, i: number) => U): U {
  return f(v, 0);
}
console.log(mapOne(5, (x: number) => x + 1));
console.log(mapOne("hi", (s: string, i: number) => s + i));

// string typevar rides through
function pick<T>(f: (a: T, b: T) => T, x: T, y: T): T {
  return f(x, y);
}
console.log(pick((a: string) => a, "left", "right"));
