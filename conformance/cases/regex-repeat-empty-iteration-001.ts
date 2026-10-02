// An iteration past a quantifier's minimum that ends where it began
// fails (RepeatMatcher), and the match goes on as if that iteration had
// never been tried: the groups keep what the last real iteration gave.

const log = (v: unknown) => console.log(JSON.stringify(v));
const show = (re: RegExp, s: string) => {
  const m = re.exec(s);
  log([String(re), s, m, m === null ? null : m.index]);
};

// a backreference after a loop whose body can match empty
show(/(?:(a)|)*\1b/, "ab");
show(/^(?:(a)|)*\1$/, "a");
show(/(?:(?:(a)|)*)*\1b/, "ab");
show(/(?:(?:(a)|)*x?)*\1b/, "ab");
show(/(?:(?=(a))|b)*\1/, "ba");

// the group itself can match empty
show(/(?:(a?))*\1b/, "ab");
show(/^(?:(a?))*\1$/, "a");
show(/^(?:(a?))*\1$/, "aa");
show(/(?:x|(a)?)*\1/, "a");

// lazy
show(/(?:(a?))*?\1b/, "ab");
show(/(?:(a)|)*?\1b/, "ab");
show(/(?:(a)|(?:))*?\1b/, "aab");
show(/^(?:(a)|b|)*?$/, "ab");

// {n,}: the iterations up to n may be empty, the ones after may not
show(/(?:(a)|){2,}\1b/, "ab");
show(/(?:(a)|){2,}\1b/, "aab");
show(/^(?:(a)|){2,}\1$/, "a");
show(/^(?:(a)|){2,}\1$/, "aa");
show(/(?:(a)|)+/, "a");
show(/(?:a|()){2}\1/, "a");

// bounded repeats check every optional iteration too
show(/(?:(a)|){0,3}/, "a");
show(/(?:(a)|){0,3}/, "aa");
show(/(?:(a)|){0,3}\1b/, "ab");
show(/(?:(a)|){1,3}/, "a");
show(/(?:(a)|){1,3}/, "");
show(/(?:(a)|){0,3}?/, "a");
show(/(?:(a)|){0,3}?b/, "ab");
show(/(?:()|a){0,2}\1b/, "ab");
show(/(?:(a)|b|){0,4}c/, "abc");
show(/(?:|a){0,2}/, "aa");
show(/(?:|a)*/, "aa");
show(/(?:(?:(a)|){0,2}c)*\1d/, "acd");

// an empty backreference inside the loop is an empty iteration too
show(/(?:(a)|\2)*(x)?/, "a");
show(/(?:(a)|b)(?:\1|c)*d/, "acd");
show(/(a)|\1/, "b");

// lookarounds as the whole body
show(/(?:(?=a)|a)*b/, "aab");
show(/(?:(?=(a)))*\1/, "a");
show(/(?:(?=(a))){0,2}\1/, "a");

// lookbehind loops walk leftwards under the same rule
show(/(?<=(?:(a)|)*)b/, "ab");
show(/(?<=(?:(a)|){0,2})b/, "ab");
show(/(?<=\1(?:(a)|)*)b/, "ab");

// g and replace see the same groups
log("ab-b".replace(/(?:(a)|)*\1b/g, "[$1]"));
log([..."aab ab".matchAll(/(?:(a)|){0,3}b/g)].map((m) => [m[0], m[1], m.index]));
