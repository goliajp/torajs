// lookbehind body captures — reverse-compiled body + reverse Pike VM
// (RFC 20260707-lookbehind-reverse-compile chunk 2). ES §22.2.2
// MatchReverse: greedy commits the LONGEST run, alternation keeps
// first-branch priority, backrefs evaluate in reverse order.

// greedy longest capture
console.log(JSON.stringify(/(?<=(\d+))px/.exec("30px")));
// lazy shortest
console.log(JSON.stringify(/(?<=(\d+?))px/.exec("30px")));
// alternation: first branch wins when both complete
console.log(JSON.stringify(/(?<=(ab|b))c/.exec("abc")));
// first branch fails -> second branch
console.log(JSON.stringify(/(?<=(a|ab))c/.exec("abc")));
// first branch wins even if shorter (backtracking order, not longest)
console.log(JSON.stringify(/(?<=(b|ab))c/.exec("abc")));
// named group
const m = /(?<=(?<num>\d+))px/.exec("30px");
console.log(m ? m.groups!.num : "nomatch");
// backref left of its group: reverse execution captures first
console.log(JSON.stringify(/(?<=\1(\w))x/.exec("aax")));
// backref right of its group: runs before capture, matches empty
console.log(JSON.stringify(/(?<=(\w)\1)x/.exec("aax")));
// anchor + greedy star captures the full prefix
console.log(JSON.stringify(/(?<=^(a*))b/.exec("aaab")));
// nested groups keep forward-oriented pairs
console.log(JSON.stringify(/(?<=((a)(b)))c/.exec("abc")));
// negative lookbehind never contributes captures
console.log(JSON.stringify(/(?<!(\d))ab/.exec("xab")));
// capture usable in replacement
console.log("30px".replace(/(?<=(\d+))px/, "[$1]"));
// global replace unaffected
console.log("30px40px".replace(/(?<=(\d+))px/g, "-"));
// test() path
console.log(/(?<=(\d+))px/.test("30px"));
// miss stays null (printed direct: JSON.stringify of a null exec
// result is a pre-existing nullable-arr stringify gap, L3b)
console.log(/(?<=(\d+))px/.exec("xxpx"));
// u-flag: cp-aware class inside lookbehind body (ASCII payload —
// non-ASCII exec-result slicing is a pre-existing gap even for
// plain /(\p{L}+)/u group 0, L3b)
console.log(JSON.stringify(/(?<=(\p{L}+))!/u.exec("ab!")));
// mutual-recursive capture/back references (test262 lookBehind)
console.log(JSON.stringify(/(?<=a(.\2)b(\1)).{4}/.exec("aabcacbc")));
console.log(JSON.stringify(/(?<=a(\2)b(..\1))b/.exec("aacbacb")));
console.log(JSON.stringify(/(?<=(?:\1b)(aa))./.exec("aabaax")));
console.log(JSON.stringify(/(?<=(?:\1|b)(aa))./.exec("aaaax")));
