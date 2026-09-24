// A typed array of pointer-shaped elements (strings, objects, arrays)
// holds a hole as an empty slot. Reading it is reading an absent
// element: undefined, the same answer as reading past the end.
let s = ["x", "y"];
s[4] = "z";
console.log(s[3], typeof s[2], s[3] === undefined, s.join("|"), 3 in s);

type P = { n: number };
let ps: P[] = [{ n: 1 }];
ps[3] = { n: 4 };
console.log(ps[1], ps[2] === undefined, ps[3].n, ps.length);

let nested: number[][] = [[1]];
nested.length = 3;
console.log(nested[2], nested[1] === undefined, nested.length);

let t = ["a", "b", "c"];
t.length = 5;
console.log(t[4], t.length, t.join("-"));
