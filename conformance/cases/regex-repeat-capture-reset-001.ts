// Every iteration of a quantified atom starts with the atom's groups
// unset (RepeatMatcher): a group that took part in an earlier
// iteration but not in the last one answers undefined.

const log = (v: unknown) => console.log(JSON.stringify(v));

log(/(?:(a)|b)*c/.exec("abc"));
log(/(?:(a)|(b))*c/.exec("abc"));
log(/(?:(x)(y)?z)*w/.exec("xyzxzw"));
log("abc".replace(/(?:(a)|b)*c/, "[$1]"));
log(new RegExp("(?:(a)|b)*c").exec("abc"));
log(/(?:(a)|b)*/.exec("aab"));
log(/(a|b)*/.exec("ab"));
log(/((a)|b)+/.exec("ab"));

// nested quantifiers
log(/(?:(?:(a)|b)+,)*/.exec("a,b,"));
log(/(?:(?:(a)|b)+,)*/.exec("b,ba,"));
log(/(?:((a)|b)+;)+/.exec("ab;ba;"));

// bounded repeats: every unrolled copy resets
log(/(?:(a)|b){2}/.exec("ab"));
log(/(?:(a)|b){2,3}/.exec("abb"));
log(/(?:(a)|b){2,3}/.exec("bba"));
log(/(?:(a)|b){0,2}c/.exec("abc"));
log(/(?:(a)|){2}/.exec("a"));

// lazy
log(/(?:(a)|b)*?c/.exec("abc"));
log(/(?:(a)|b)+?$/.exec("ab"));

// sticky
const y = /(?:(a)|b)*c/y;
log(y.exec("abc"));
log(y.lastIndex);
y.lastIndex = 1;
log(y.exec("abc"));

// g: matchAll and the replace callback's arguments
log([..."abc-bac-abbc".matchAll(/(?:(a)|b)*c/g)].map((m) => [m[0], m[1], m.index]));
log("abc-bac".replace(/(?:(a)|(b))*c/g, (m: string, g1: string, g2: string) => m + ":" + g1 + ":" + g2));
log("abc-bac".replace(/(?:(a)|b)*c/g, "[$1]"));
log("abc-bac".match(/(?:(a)|b)*c/g));

// a backreference to a reset group matches empty
log(/(?:(a)|b\1)*/.exec("abab"));
log(/(?:(a)|b\1)*/.exec("abaab"));
log(/(?:(a)|b\1)+x/.exec("abax"));
log(/^(?:(a)|b)*\1$/.exec("abb"));
log(/^(?:(a)|b)*\1$/.exec("ba"));

// named groups
const n = /(?:(?<A>a)|(?<B>b))*c/.exec("abc");
log(n);
log(n !== null && n.groups !== undefined ? [n.groups.A === undefined, n.groups.B] : null);
log("abc".replace(/(?:(?<A>a)|b)*c/, "[$<A>]"));

// u flag
log(/(?:(\u{1F600})|b)*c/u.exec("\u{1F600}bc"));
log(/(?:(a)|b)*c/u.exec("abc"));
log(/(?:(é)|b)*c/u.exec("ébc"));

// lookbehind walks its loop leftwards: the leftmost iteration is the last
log(/(?<=(?:(a)|b)*)c/.exec("abc"));
log(/(?<=(?:(a)|b)*)c/.exec("bac"));

// a group around the quantifier is not inside it
log(/a(b+)c/.exec("abbc"));
log(/([a-z]+)=(\d+)/.exec("key=42"));
