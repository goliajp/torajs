// rotation 638 — §14.7.5: a bare for-in head that opens with `[` but is an
// INDEXED literal (`[arr][0][1]`) is a MemberExpression target assigned each
// iteration, not an assignment pattern. oracle: node script goal.
var obj = { key: 1 };
var arr: any = [0, 0];
for ([arr][0][1] in obj) ;
console.log("index", arr[1]);
var o: any = { p: 0 };
for ({ q: o }.q.p in obj) ;
console.log("member", o.p);
