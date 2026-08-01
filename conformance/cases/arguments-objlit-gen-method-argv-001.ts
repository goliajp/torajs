// RFC 20260801-arguments-method-face knife 4d — an object-literal
// generator method's arguments ride the GEN_ARGV_PARAM channel
// (mirroring the class-side knife 2b): the parser renames body
// `arguments` to the trailing argv param, and the `__forward_` relay
// fills that slot with its own [...arguments] instead of declaring
// it — so the true call-site argv reaches the factory through the
// relay's argv/static face, direct call and escaped alias alike.

// direct member call, over-arity
var o1 = {
  *gm() {
    console.log(arguments.length, arguments[0], arguments[1]);
    yield 1;
  },
};
o1.gm(42, "TC39").next();

// escaped alias, bare call
var o2 = {
  *gm() {
    console.log(arguments.length, arguments[0], arguments[1]);
    yield 1;
  },
};
var ref2 = o2.gm;
ref2(7, "x").next();

// async generator, escaped alias
var o3 = {
  async *am() {
    console.log(arguments.length, arguments[0], arguments[1]);
    yield 1;
  },
};
var ref3 = o3.am;
ref3(9, "y")
  .next()
  .then(() => console.log("done"));

// arguments read across a yield resumption
var o4 = {
  *gm(a: number) {
    yield a;
    console.log(arguments.length, arguments[1]);
  },
};
var it = o4.gm(5, "after");
it.next();
it.next();
