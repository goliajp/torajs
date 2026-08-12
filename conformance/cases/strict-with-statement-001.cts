// §14.11.1 — a WithStatement is a SyntaxError in strict mode code.
// `with` reaches the parser as a plain identifier, so the recognition
// is "the name, followed by `(`, in statement position". This fixture
// guards everything that spelling must NOT swallow: `with` as a
// property name is an ordinary member, in either goal.
const arr = [1, 2, 3];
console.log(arr.with(0, 9).join(","));

const o = { with: (x: number) => x + 1 };
console.log(o.with(6));

const holder: any = {};
holder.with = 4;
console.log(holder.with);

// A statement that merely STARTS with something ending in `with` is
// not the form either.
const nowith = (n: number) => n * 2;
console.log(nowith(5));
