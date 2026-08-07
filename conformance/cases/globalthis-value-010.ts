// bare `globalThis` as a VALUE — RFC 20260807-global-object G2: one
// immortal singleton (Math ns-object lane) pre-filled with the ctor
// cells and §19.1.1 value props; identity holds across every read,
// and the dynamic lane answers the same interned identities the bare
// names do. Mutations through bare globalThis stay loud (compile
// reject); known-but-unfilled builtins throw on the dynamic lane.
var g = globalThis;
console.log(typeof g);
console.log(globalThis === globalThis);
console.log(g === globalThis);
console.log(g.Array === Array);
console.log(g["Math"] === Math);
console.log(typeof g.NaN);
console.log(g.Infinity === Infinity);
console.log(g.undefined === undefined);
console.log(g.globalThis === g);
console.log("after");
