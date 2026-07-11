// RFC 20260711 follow-up — multi-arg console.log borrow judgement
// was missing the Index / OptChain / This arms (the single-arg lane
// gained them in chunk 570): container element reads were treated
// as fresh temps and dropped without a matching inc, freeing the
// element under the still-live array (the next join's output block
// recycled the cell: 'x1|x1'). Static-literal elements masked it
// (their rc ops are no-ops).
const m: string[] = [];
m.push("x".concat("1"));
m.push("y".concat("2"));
console.log(m[0], m[1]);
console.log(m.join("|"));
const e = Array.from("汉a字");
console.log(e.length, e[0], e[1], e[2]);
console.log(e.join("|"));
const g = Array.from("ab");
console.log(g[0], g[1]);
console.log(g.join("|"));
console.log(g[0], g[1]);
const single = ["p".concat("q")];
console.log(single[0]);
console.log(single.join(""));
