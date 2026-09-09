// §12.7.6 PropertyName is an IdentifierName. Two slots read a shorter
// set: the object literal's async shorthand matched `Token::Ident`
// alone, so `{ async if(){} }` fell back to the field path and
// demanded a `:` after the word `async` (the generator spelling
// `{ async *if(){} }` already worked); and the destructuring field
// slot asked the BINDING admission even when a `:` followed, so
// `var { let: x } = o` parked `let` for the module-goal gate and a
// module that node runs was refused — the word there names the FIELD.
var m = {
  async iff() { return 1; },
  async let() { return 2; },
  async yield() { return 3; },
  async async() { return 4; },
  async await() { return 5; },
  async type() { return 6; },
  async class() { return 7; },
  async default() { return 8; },
};
Promise.all([m.iff(), m.let(), m.yield(), m.async(), m.await(), m.type(), m.class(), m.default()])
  .then(function (vs) { console.log(vs.join(",")); });

var src = { let: 9, yield: 10, await: 11, if: 12, class: 13 };
var { let: a, yield: b, await: c, if: d, class: e } = src;
console.log(a, b, c, d, e);

// The shorthand spellings still bind in a script, and a default still
// fires on the shorthand.
var { let, yield } = { let: 14, yield: 15 };
console.log(let, yield);
var { let: f = 16 } = {};
console.log(f);

// A member actually named `async` is unchanged.
var g = { async: 17, asyncFn: function () { return 18; } };
console.log(g.async, g.asyncFn());
