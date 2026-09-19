// A pattern that is one run of literal ASCII bytes searches as a
// substring find, skipping the DFA and the Pike VM. Every entry that
// searches must see the same answer: global and sticky walks,
// lastIndex, split / replace / test / search / matchAll, misses,
// ignore-case (excluded from the fast path) and capture groups.

// global / not whole
let gl: string[] | null = "xax".match(/x/g);
if (gl !== null) console.log(gl.join(","), gl.length);
let whole: string[] | null = "foo".match(/foo/g);
if (whole !== null) console.log(whole.join(","));
// sticky + lastIndex
let st: RegExp = /o/y;
st.lastIndex = 1;
console.log(st.test("foo"), st.lastIndex);
st.lastIndex = 0;
console.log(st.test("foo"), st.lastIndex);
let sg: RegExp = /o/gy;
console.log("oofoo".replace(sg, "0"));
let gi: RegExp = /ab/g;
console.log(gi.exec("xxabab")!.index, gi.lastIndex, gi.exec("xxabab")!.index, gi.lastIndex, gi.exec("xxabab"), gi.lastIndex);
// literal fast path through the other entries
console.log("a,b,,c".split(/,/).join("|"), "a--b--c".split(/--/).length);
console.log("hello world".replace(/o/, "0"), "hello world".replace(/o/g, "0"), "aaa".replace(/aa/g, "b"));
console.log(/ll/.test("hello"), /lx/.test("hello"), "hello".search(/lo/), "hello".search(/z/));
let n: number = 0;
for (const mm of "abcabcab".matchAll(/abc/g)) n += mm.index!;
console.log(n);
console.log("ababab".replace(/aba/g, "X"), "".match(/a/), "a".match(/ab/), "ab".match(/abc/));
console.log("xAx".match(/a/), "xAx".match(/a/i)![0], /\./.test("a.b"), "a.b".split(/\./).length);
// capture group literals keep going through the VM
let cg: string[] | null = "key=val".match(/(key)=(val)/);
if (cg !== null) console.log(cg[1], cg[2], cg.index);
