// A one-pass matcher whose start state enters one state on a class
// and that state loops on exactly the same class resumes a dead scan
// where the run ended instead of one unit after the start. Every
// shape below either takes that resume or is one the resume must
// decline, and the leftmost match must come out the same either way.

// the resume shape: a letter run that never reaches `=` or a digit
// is skipped whole, and the match after it still starts at its first
// letter
console.log(JSON.stringify("abc;x=1".match(/([a-z]+)=(\d+)/)));
console.log(JSON.stringify("ab=x cd=12".match(/([a-z]+)=(\d+)/)));
console.log(JSON.stringify("ab=cd=ef=1".match(/([a-z]+)=(\d+)/)));
console.log(JSON.stringify("a=1".match(/([a-z]+)=(\d+)/)));
console.log(JSON.stringify("===".match(/([a-z]+)=(\d+)/)));
console.log(JSON.stringify("abc".match(/([a-z]+)=(\d+)/)));
console.log(JSON.stringify("abc=".match(/([a-z]+)=(\d+)/)));
console.log(JSON.stringify("".match(/([a-z]+)=(\d+)/)));
console.log(JSON.stringify("x=1;yy=22;zzz;w=3".match(/([a-z]+)=(\d+)/g)));

// the global scan restarts at lastIndex, inside a run
const re = /([a-z]+)=(\d+)/g;
re.lastIndex = 1;
console.log(JSON.stringify(re.exec("abc=1;de=2")));
console.log(re.lastIndex);
re.lastIndex = 2;
console.log(JSON.stringify(re.exec("abcdef;de=2")));
console.log(re.lastIndex);

// a run whose exit state dies later: `=` is reached, the digit is not
console.log(JSON.stringify("key=;k2=7".match(/([a-z]+)=(\d+)/)));
console.log(JSON.stringify("a=b=c=9".match(/([a-z]+)=(\d+)/)));

// the resume must not move a match that starts inside a run: `x` is
// in the class the run loops on, so this is not the resume shape
console.log(JSON.stringify("aaaxb".match(/([a-z]+)x/)));
console.log(JSON.stringify("aaaxb".match(/([a-y]+)x/)));
console.log(JSON.stringify("xxab".match(/(a|ab)c/)));
console.log(JSON.stringify("ababc".match(/(a|ab)c/)));

// a repetition whose body begins with the run's class: the repeat
// re-enters a copy of the body, not the run state, so the end the
// scan resumes from is still the first run's, and the match that
// starts inside that run is found
console.log(JSON.stringify("ab=cd=xyzr=5kX".match(/(?:[a-z]+=[a-z]+=[a-z0-9][a-z])+X/)));
console.log(JSON.stringify("ab=cd=xyzr=5kX".match(/(?:[a-z]+=[a-z]+=[a-z0-9][a-z])+X/g)));
console.log(JSON.stringify("xx ab=cd=ef=1=2 yy".match(/[a-z]+(?:=[a-z0-9]+)+/g)));

// a repetition after the run that never re-enters the run state
console.log(JSON.stringify("aa=bb=ccd=12; q=3".match(/([a-z]+)(?:=\d+)+/)));
console.log(JSON.stringify("aa=bb=ccd=12; q=3".match(/([a-z]+)(?:=\d+)+/g)));

// word boundaries probe the position, so the start state is not a
// plain class entry
console.log(JSON.stringify("foobar=1 bar=2".match(/\b([a-z]+)=(\d+)/)));
console.log(JSON.stringify("foobar=1 bar=2".match(/([a-z]+)\b=(\d+)/)));
console.log(JSON.stringify("ab cd=1".match(/\b([a-z]+)=(\d+)\b/g)));

// sticky: one attempt at lastIndex, no scan to resume
const sticky = /([a-z]+)=(\d+)/y;
console.log(JSON.stringify(sticky.exec("ab;c=1")));
console.log(sticky.lastIndex);
sticky.lastIndex = 3;
console.log(JSON.stringify(sticky.exec("ab;c=1")));
console.log(sticky.lastIndex);

// the run state also accepts: a dead attempt there is an answer
console.log(JSON.stringify("  hello  ".match(/[a-z]+/)));
console.log(JSON.stringify("x1y22z".match(/\d+/g)));

// unicode classes over surrogate pairs: the run steps by code point
console.log(JSON.stringify("日本語;漢字=12".match(/(\p{L}+)=(\d+)/u)));
console.log(JSON.stringify("𝒜𝒝;𝒞=7".match(/(\p{L}+)=(\d+)/u)));
console.log(JSON.stringify("𝒜𝒝𝒞".match(/(\p{L}+)=(\d+)/u)));
console.log(JSON.stringify("ab=𝒞 𝒟=3".match(/([^=\s]+)=(\d+)/u)));
console.log(JSON.stringify("αβγ δε=5".match(/(\p{L}+)=(\d+)/gu)));

// wide subject without the u flag: units, not code points
console.log(JSON.stringify("日本語;漢字=12".match(/([^=;]+)=(\d+)/)));
console.log(JSON.stringify("𝒜𝒝;𝒞=7".match(/([^=;]+)=(\d+)/g)));

// case folding widens the class the run loops on
console.log(JSON.stringify("ABc;De=1".match(/([a-z]+)=(\d+)/i)));
console.log(JSON.stringify("abC=1".match(/([a-z]+)=(\d+)/i)));

// replace and split walk the same matcher
console.log("aaa bb=1 ccc d=2".replace(/([a-z]+)=(\d+)/g, "<$1:$2>"));
console.log(JSON.stringify("aaa bb=1 ccc d=2".split(/([a-z]+)=(\d+)/)));
