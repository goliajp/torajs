// chunk 743 - substr views through push/unshift materialize to owned
// Str (pre-fix unshift stored the raw view pointer into a Str slot -
// join/print walked garbage past the header) and the coerce lane
// releases a fresh view's own ref (borrow views stay with their owner)
const s = "hello";
const ss: string[] = ["z"];
ss.unshift(s[1]);
ss.unshift(s.slice(0, 2));
console.log(ss.join(","));
console.log(ss[0], ss[1], ss[1].length);

const v = s.slice(1, 3);
const ts: string[] = [];
ts.push(v);
ts.unshift(v);
console.log(ts.join(","), v);
ts.push(s[0]);
ts.unshift(s.slice(3));
console.log(ts.join(","));
console.log(s);
