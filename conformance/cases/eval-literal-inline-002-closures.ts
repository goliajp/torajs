// eval inside a function written in EXPRESSION position.
// The statement walk reaches a `function` declaration because that
// hangs off the statement tree; an arrow, a function expression and a
// method shorthand all park their bodies in the expression arena
// instead. test262 spells most of its eval assertions as
// `assert.throws(…, function () { eval("…") })`, so this shape carries
// a large share of the corpus.

const arrow = () => {
  eval("console.log('arrow body');");
};
arrow();

const fnExpr = function () {
  eval("console.log('function expression body');");
};
fnExpr();

const obj = {
  m() {
    eval("console.log('method shorthand body');");
  },
};
obj.m();

// nested one level down: an arrow inside an arrow
const outer = () => {
  const inner = () => {
    eval("console.log('nested arrow body');");
  };
  inner();
};
outer();

// an arrow that the eval'd source itself declares and calls
eval("const made = () => { console.log('arrow made inside eval'); }; made();");

// a declaration inside the arrow's eval stays inside it
const scoped = () => {
  eval("let only = 'local to this eval'; console.log(only);");
};
scoped();

// eval in a callback passed to an array method
[1, 2].forEach(function () {
  eval("console.log('callback body');");
});
