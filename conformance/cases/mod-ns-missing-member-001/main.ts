// §10.4.6.8 — a module namespace exotic object answers `undefined`
// for a non-exported name (421-05). Two shapes: a name the module
// never exported, and `default` behind `export * from` (which never
// forwards it, §16.2.3). The namespace is not extensible, so the
// checker answers Undefined statically instead of the
// anonymous-struct typo reject.
import * as ns from "./hub";
console.log(ns.default);
console.log(typeof ns.notThere);
console.log(ns.x);
const d = ns.default;
console.log(d === undefined);
