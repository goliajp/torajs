// a typed-parameter reaction handler may return a heap value; the
// derived promise carries it like the any-parameter handlers' returns
Promise.resolve(1).then((v: number) => [v, v]).then((a) => console.log("arr", a.length, a[1]));
Promise.resolve(1).then((v: number) => [v, v]).then((a: number[]) => console.log("arr-typed", a.length));
Promise.resolve([1, 2]).then((a: number[]) => a.map((x) => x * 2)).then((b) => console.log("map", b));
Promise.resolve(1).then((v: number) => ({ n: v })).then((o) => console.log("struct", o.n));
Promise.resolve("x").then((s: string) => [s]).then((a) => console.log("str-arr", a[0]));
class K { constructor(public n: number) {} }
Promise.resolve(1).then((v: number) => new K(v)).then((k) => console.log("class", k.n));
Promise.resolve(1).then((v: any) => [v, v]).then((a) => console.log("any-arr", a.length, a[1]));
Promise.reject(new Error("r")).catch((e: any) => [e.message]).then((a) => console.log("catch-arr", a[0]));
