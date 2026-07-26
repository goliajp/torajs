// An arrow whose return annotation is an inline function type did not
// parse: the arrow-vs-parenthesized-expression lookahead required the
// annotation to start with an identifier, so the leading `(` of
// `(b: number) => void` read as "not an arrow" and the whole
// declaration fell into the expression path, dying on the empty `()`.

const mk = (): (b: number) => void => {
  return (b: number) => {
    console.log(b);
  };
};
mk()(3);

// A returning one.
const mk2 = (): (b: number) => number => {
  return (b: number) => b + 1;
};
console.log(mk2()(41));

// Zero parameters, and more than one.
const mk3 = (): () => number => {
  return () => 5;
};
console.log(mk3()());

const mk4 = (): (a: number, b: number) => number => {
  return (a: number, b: number) => a * b;
};
console.log(mk4()(6, 7));

// The spellings that already worked stay working.
type Cb = (b: number) => void;
const viaAlias = (): Cb => {
  return (b: number) => {
    console.log(b + 100);
  };
};
viaAlias()(3);

function viaDecl(): ((b: number) => void) {
  return (b: number) => {
    console.log(b + 1000);
  };
}
viaDecl()(3);

const held: ((b: number) => void) = (b: number) => {
  console.log(b + 10000);
};
held(3);

// Ordinary return annotations are untouched.
const plain = (): number => 7;
const voided = (): void => {
  console.log("v");
};
const arr = (): number[] => [1, 2];
console.log(plain(), arr()[1]);
voided();
