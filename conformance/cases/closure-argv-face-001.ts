// full-arguments closure tier (RFC 20260708-closure-argv-face
// chunk 1): a closure VALUE whose body reads arguments[i] rides the
// boxed dual entry — the adapter feeds real argc + raw argv, the
// body materializes __torajs_arguments from them, and the public
// type is (...args: any[]) => R so any call arity admits.
const f = function () { return arguments[0] + arguments[1]; };
console.log(f(40, 2));
const g = function () {
  let s = 0;
  for (let i = 0; i < arguments.length; i++) { s += arguments[i]; }
  return s;
};
console.log(g(1, 2, 3, 4));
console.log(g(1, 2, 3, 4, 5, 6, 7, 8, 9, 10));
console.log(g());
const bare = function () { return arguments[1]; };
console.log(bare("x", "a genuinely long heap string beyond shortstr"));
const alias = bare;
console.log(alias(1, 7));
