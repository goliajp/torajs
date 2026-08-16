// §22.1.3.19 String.prototype.replace hands a function replaceValue
// to `Call(replaceValue, undefined, «matched, …, string»)`, and the
// regexp route it delegates to says the same thing (§22.2.6.11
// RegExp.prototype[@@replace] step 14.a). So a function EXPRESSION
// used as the replacer reads `this` as undefined.
//
// tr refused to compile it instead: the receiver-promoting knife
// declines a regexp searchValue (that lowering lane has no receiver
// slot, unlike the string-searchValue one), and a declined `this`
// leaves an unbound `__this` capture. The answer is not a promotion —
// it is the no-receiver slot table, which binds the body's `this` to
// undefined locally and leaves the closure's ABI alone.

console.log("x".replace(/x/, function (m: string) {
  return typeof this;
}));

console.log("a-b-c".replaceAll(/-/g, function (m: string) {
  return this === undefined ? "|" : "?";
}));

// the matched text and the capture still arrive as written
console.log("john smith".replace(/(\w+)\s(\w+)/, function (
  m: string,
  first: string,
  last: string,
) {
  return last + ", " + first + ":" + (typeof this);
}));

// the string-searchValue spelling answers the same thing by the other
// route (that one promotes, and the call site seeds undefined)
console.log("y".replace("y", function (m: string) {
  return typeof this;
}));
