// builtin namespace statics handed over as callbacks a typed consumer calls
const a = [1, 4, 9];
console.log(a.map(Math.sqrt).join(","), [1, 2.5, 3].filter(Number.isInteger).join(","), [[1], 2].map(Array.isArray).join(","));
[7, 8].forEach(console.log);
console.log([9, 4, 0].map(Math.max).join(","), [1, 2, 3].reduce(Math.max));
Promise.resolve(16).then(Math.sqrt).then((v) => console.log("then", v));
Promise.resolve({ k: 1 }).then(JSON.stringify).then((s) => console.log(typeof s, s));
Promise.resolve("p").then(console.log);
const h = console.log;
h("bound");
console.log(Math.max.length, Math.max.name, console.log.name);
