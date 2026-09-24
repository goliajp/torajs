// A string element read past the end (or a find miss) is undefined, and
// stays undefined when it is stored into an any[] by push, unshift or
// index assignment — not the string "undefined".
var s = ["x"];
var o: any[] = [];
o.push(s[5]);
o.push(s[0]);
o[2] = s[9];
o.unshift(s[7]);
console.log(typeof o[0], typeof o[1], o[2], typeof o[3], o.join("|"), o.length);
var n: any[] = [null as any];
var t: (string | null)[] = [null];
n.push(t[0]);
console.log(n[1] === null);
