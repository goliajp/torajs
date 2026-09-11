// §12.7.2 — a ReservedWord written with a unicode escape is never the
// keyword, but a BindingIdentifier is judged on its StringValue, so
// the escaped spelling may name something exactly where the bare one
// may, and nowhere else. The lexer hands these over as a separate
// token so each position can opt in; four positions had not, and each
// one refused a program node runs.
//
// Oracle is node, pinned in `.expected`: bun rejects an escaped
// `await` as a name even in a CommonJS entry.
function decls() {
  var \u0079ield = 1;
  var \u0061wait = 2;
  return \u0079ield + \u0061wait;
}
console.log("var-decl ", decls());

function shorthandPattern() {
  var { \u0061wait } = { await: 4 };
  return \u0061wait;
}
console.log("shorthand", shorthandPattern());

function renamed() {
  var { \u0061wait: other } = { await: 5 };
  return other;
}
console.log("rename   ", renamed());

function className() {
  class \u0061wait {}
  return new (\u0061wait)() instanceof \u0061wait;
}
console.log("class    ", className());

\u0079ield: {
  console.log("label-y  ", "reached");
  break \u0079ield;
}
\u0061wait: {
  console.log("label-a  ", "reached");
}

// A parameter and a catch binding are the same position.
function param(\u0079ield) {
  return \u0079ield;
}
console.log("param    ", param(6));
try {
  throw 7;
} catch (\u0061wait) {
  console.log("catch    ", \u0061wait);
}
