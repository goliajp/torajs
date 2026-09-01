// 551-03 (first half) — a tag function takes the template object.
// `__torajs_template_object` answers a nanboxed Any (the runtime cache
// hands back a frozen array with `.raw` wired on), so the strings
// argument arrives as `Any` against a `string[]` param, and TS
// any-assignability routes that through the any-widen monomorph lane:
// the callee is cloned with that one param widened and the call site
// retargets to the clone.
//
// The lane refused every tag function because its gate asked that NO
// parameter be rest — but a tag function always has one, for the
// substitutions. Only the slot being widened has to be non-rest (its
// annotation becomes a scalar `any`); the rest slot rides the clone
// untouched, and that shape — `(strs: any, ...vals: any[])` — is one
// the lane already serves when spelled by hand.
//
// Still refused, and tracked as the second half: the same tag written
// as `const tag = (...) => ...`, which the lane excludes for being a
// lifted closure.
function tag(strs: string[], ...vals: any[]): string {
  return strs.join("|") + "#" + vals.length;
}

const x = 1;
const y = "b";
console.log(tag`a${x}b${y}c`);
console.log(tag`plain`);
console.log(tag`${x}`);
