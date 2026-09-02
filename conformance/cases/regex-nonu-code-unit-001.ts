// 558-01 — without `u` / `v` a pattern matches over UTF-16 code units
// (§22.2.2.1): `.` and a class step one unit, a surrogate pair is two.
// The haystack and the pattern transcode to the code-unit form; `u`
// merges pairs into code points (str_helpers::haystack).
const e = "😀";
console.log(e.match(/./g)!.length, /^.$/.test(e), /^..$/.test(e), /^.$/u.test(e), /^..$/u.test(e));
console.log(e.replace(/./g, "_"), e.replace(/./gu, "_"));
console.log(JSON.stringify(/[^x]/.exec(e)), JSON.stringify(/[^x]/u.exec(e)));
console.log(JSON.stringify(e.match(/[^x]/g)), e.match(/[^x]/g)!.length, e.match(/[^x]/gu)!.length);
console.log(/😀/.test(e), /😀/u.test(e), /^😀$/.test(e), JSON.stringify(/😀/.exec("a😀b")), /😀/.exec("a😀b")!.index);
console.log(/😀/.test(e), /\uD83D/.test(e), /\uDE00/.test(e), /\uD83D/u.test(e), /\uDE00/u.test(e));
console.log(/[😀]/.test("\uD83D"), /[😀]/u.test("\uD83D"), /^[😀]$/.test(e), /^[😀]$/u.test(e), /^[😀]{2}$/.test(e));
console.log("x😀y".split(/(?:)/).length, "x😀y".split(/(?:)/u).length);
console.log(JSON.stringify("x😀y".split(/(?:)/)), JSON.stringify("x😀y".split(/(?:)/u)));
const g = /./g; g.lastIndex = 1; console.log(JSON.stringify(g.exec(e)), g.lastIndex);
const gu = /./gu; gu.lastIndex = 1; console.log(JSON.stringify(gu.exec(e)), gu.lastIndex);
console.log(JSON.stringify([..."a😀b".matchAll(/./g)].map((m) => m.index)), JSON.stringify([..."a😀b".matchAll(/./gu)].map((m) => m.index)));
console.log(JSON.stringify("a😀b".replace(/(.)(.)/, "$2$1")), JSON.stringify("a😀b".replace(/(.)(.)/u, "$2$1")));
console.log(JSON.stringify("a😀b".replace(/(.)(.)/, (_m, p1, p2) => p2 + "|" + p1)), JSON.stringify("a😀b".replace(/(.)(.)/u, (_m, p1, p2) => p2 + "|" + p1)));
console.log(new RegExp("😀").source, new RegExp("😀", "u").source, new RegExp("😀").source.length, new RegExp("😀", "u").source.length);
console.log(new RegExp("😀").test(e), new RegExp("😀", "u").test(e), new RegExp(/😀/, "u").test(e), new RegExp(/😀/u, "").test(e));
console.log(new RegExp("^.$").test(e), new RegExp("^.$", "u").test(e), new RegExp(/^.$/, "u").test(e), new RegExp(/^.$/u, "").test(e));
console.log("😀".search(/\uDE00/), "😀".search(/\uDE00/u), JSON.stringify("😀".replaceAll(/\uDE00/g, "!")), "😀".replaceAll(/\uDE00/gu, "!"));
console.log("😀😀".lastIndexOf("\uDE00"), JSON.stringify("😀😀".match(/\uDE00/g)), JSON.stringify("😀😀".match(/\uDE00/gu)));
console.log(/^\S$/.test(e), /^\S\S$/.test(e), /^\S$/u.test(e), /^\W\W$/.test(e), /^\W$/u.test(e));
console.log(/^.$/i.test(e), /^..$/i.test(e), /^.$/s.test(e), /^..$/s.test(e), /^.$/su.test(e));
console.log(JSON.stringify(/(?<a>.)(?<b>.)/.exec(e)!.groups), JSON.stringify(/(?<a>.)/u.exec(e)!.groups));
console.log(/(.)\1/.test("\uD83D\uD83D"), /(.)\1/.test(e), /(.)\1/u.test(e), /(.)\1/u.test("😀😀"));
console.log("xéy".split(/(?:)/).length, "x日y".split(/(?:)/).length, JSON.stringify("é日".split(/(?:)/)), JSON.stringify("aé".split(/(?:)/y)));
