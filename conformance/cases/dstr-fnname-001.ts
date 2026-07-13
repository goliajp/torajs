// RFC 20260714-dstr-residual blade 2 — ES §8.4.5 NamedEvaluation for
// destructuring defaults: an anonymous function definition in default
// position takes the binding identifier as its `.name`; a named
// function expression keeps its self-name (§15.5.5).

// array pattern, param position
function fa([fn = function () {}, xFn = function x() {}]) {
  console.log(fn.name, "|", xFn.name, "|", fn.length);
}
fa([]);

// object pattern, param position — shorthand + self-named
function fo({ cb = () => 1, named = function self() {} }: any) {
  console.log(cb.name, "|", named.name);
}
fo({});

// object pattern, param position — rename target
function fr({ a: renamed = function () {} }: any) {
  console.log(renamed.name);
}
fr({});

// let position, object pattern — plain + parenthesized
let src: any = {};
let { g = function () {}, h = (function () {}) } = src;
console.log(g.name, "|", h.name);

// let position, array pattern
function fk([k = () => 2]: any) {
  console.log(k.name);
}
fk([]);

// default NOT taken — the passed value wins over the named default
// (a passed FN value through the default ternary hits a pre-existing
// reified-fn repr gap, recorded in RFC 20260714-dstr-residual; a
// primitive locks the not-taken semantics here)
function ft({ t = () => 1 }: any) {
  console.log(typeof t, t);
}
ft({ t: 5 });
console.log("done");
