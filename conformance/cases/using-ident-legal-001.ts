// Legal faces around the new loud rejects: `using` stays an ordinary
// identifier outside a declaration head (index base, member name,
// plain reads), a rest element WITHOUT a trailing comma stays a valid
// destructuring-assignment tail, and an expression-position spread
// with a trailing comma stays a plain array literal.
let using: any = [1, 2, 3];
console.log(using[1]);
const obj: any = { using: (x: any) => x * 2 };
console.log(obj.using(3));
console.log(using.length);

let a: any;
let b: any;
[a, ...b] = [1, 2, 3];
console.log(a, b);

const src: any = [4, 5];
const spread_ok: any = [...src, ];
console.log(spread_ok);
