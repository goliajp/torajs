// A quantifier at the very start of a pattern loops back to the
// pattern's start, so a match walks through its first position more
// than once. Captures from the iterations before the last must stay.

const log = (v: unknown) => console.log(JSON.stringify(v));

log(/(?:(a)b)*c/.exec("abc"));
log(/(?:(a)b)*c/.exec("ababc"));
log(/(?:(a)b)*c/.exec("c"));
log(/(?:(a)b)*c/.exec("xxababcab"));
log(/(?:(a)b)*/.exec("ababx"));
log("abcababc".match(/(?:(a)b)*c/g));
log([..."abc-ababc-c".matchAll(/(?:(a)b)*c/g)].map((m) => [m[0], m[1], m.index]));
log("abc-ababc-c".replace(/(?:(a)b)*c/g, "[$1]"));
log("ababc".replace(/(?:(a)b)*c/, (m: string, g1: string) => m + ":" + g1));

// nested groups, one of them optional
log(/(?:(x)(y)?z)*w/.exec("xzxyzw"));
log(/(?:(x)(y)?z)*w/.exec("w"));
log("xyzw xzw".replace(/(?:(x)(y)?z)*w/g, "<$1|$2>"));

// sticky, case-insensitive and unicode variants
const y = /(?:(a)b)*c/y;
log(y.exec("ababc"));
log(y.lastIndex);
y.lastIndex = 1;
log(y.exec("ababc"));
log(/(?:(a)b)*c/i.exec("ABabC"));
log(/(?:(a)b)*c/u.exec("ababc"));
log(/(?:(\u{1F600})b)*c/u.exec("\u{1F600}b\u{1F600}bc"));
log(/(?:(a)b)*c/giu.exec("xAbABc"));

// alternatives inside the loop: the last iteration takes the group
log(/(?:(a)|b)*c/.exec("bac"));
log(/(?:(a)|(b))*c/.exec("bbc"));
log("bac".replace(/(?:(a)|b)*c/, "[$1]"));
