// A whole-string match answers the subject itself as m[0] (one more
// share, no copy). The share must be invisible: appending to m[0]
// leaves the subject alone, capture groups and split views as the
// receiver still answer their own text, and non-ASCII subjects keep
// the copying path.

// whole-string hit shares the receiver: appending to m[0] must not touch s
let s: string = "x";
let m: string[] | null = s.match(/x/);
if (m !== null) {
  let t: string = m[0];
  t += "yz";
  console.log(s, t, m[0], m[0] === s, m.length);
}
let built: string = "ab" + "c";
let mb: string[] | null = built.match(/abc/);
if (mb !== null) { let u: string = mb[0]; for (let i: number = 0; i < 3; i++) u += "!"; console.log(built, u, mb[0]); }
// whole-string capture group
let g: any = /(abc)/.exec("abc");
if (g !== null) console.log(g[0], g[1], g.index, g.input);
let g2: string[] | null = "abc".match(/(a)(bc)/);
if (g2 !== null) console.log(g2.join("|"));
// split view as receiver
let parts: string[] = "aa bb cc".split(" ");
let mv: string[] | null = parts[1].match(/bb/);
if (mv !== null) console.log(mv[0], mv[0].length, parts.join(","));
let mv2: string[] | null = parts[2].match(/c/);
if (mv2 !== null) console.log(mv2[0], mv2.index);
// non-ascii receiver
let e: string[] | null = "é".match(/é/u);
if (e !== null) console.log(e[0], e[0].length);
let e2: string[] | null = "aé".match(/a/);
if (e2 !== null) console.log(e2[0], e2.index);
