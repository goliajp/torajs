// §22.1.3.19 / §22.2.6.11 run ToString on the replacer's result. A
// callback whose body is `x as any` takes its contextual `=> string`
// return type, and `as any` lowers to x's own representation: the
// array came back as a Str pointer (`a` + garbage + `c`).
console.log("abc".replace(/b/, () => [9, 8] as any));
console.log("abc".replace("b", () => [9, 8] as any));
console.log("abcb".replaceAll("b", (m: string) => [m, m] as any));
console.log("abc".replace(/b/g, () => ({ toString() { return "Q"; } }) as any));
for (let i = 0; i < 3; i++) console.log("xyx".replace(/y/, () => [i, [i]] as any));
// A callback that DECLARES an any return goes through the boxed call.
console.log("abc".replace(/b/, (): any => [7, 6]), "abc".replace(/b/g, (m: string): any => m + 1));
console.log("abc".replace("b", (): any => [5]));
