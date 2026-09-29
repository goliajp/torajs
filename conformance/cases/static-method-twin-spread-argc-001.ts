// A static method whose twin took the value lane (a boolean slot, or a
// body reading `this`) at a direct spread call site. The wrap pass
// hands the site to the `__forward_` relay, and the relay forwards to
// the receiver-polymorphic twin under the mono's name; the hidden
// argc slot that relay fills has to carry the relay's own runtime
// count, not the twin's declared arity. `two(true, "x", ...arr)` read
// `arguments.length` as 2 while the untyped `plain` beside it read 4.
// One and two boolean slots, arguments index reads past the arity,
// and the direct call without a spread.
class E {
  static two(a: boolean, b: string) { return [a, b, arguments.length, arguments[3]]; }
  static one(a: boolean) { return [a, arguments.length, arguments[2]]; }
  static plain(a, b) { return [a, b, arguments.length]; }
}
const arr = [1, 2];
console.log(JSON.stringify(E.two(true, "x", ...arr)));
console.log(JSON.stringify(E.one(true, ...arr)));
console.log(JSON.stringify(E.plain(1, 2, ...arr)));
console.log(JSON.stringify(E.two(true, "x")), JSON.stringify(E.one(false)));
console.log(JSON.stringify(E.two(...([false, "y"] as any[]))));
