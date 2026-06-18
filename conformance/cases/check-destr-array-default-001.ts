// Array destructuring `=` default per ES §13.3.3 BindingElement Init —
// counterpart to the object-destructuring default lowering shipped at
// 18210dbe. Pre-fix: parser rejected `=` after the binding identifier
// in `let [a = 1] = arr` with "expected `]` ending array destructuring,
// got Eq".
//
// Implementation:
//   * parser.rs::parse_array_destructuring entries now carry an
//     Option<ExprId> default expr per slot (mirrors the object-destr
//     entry shape), parsed via parse_assign when `=` follows the
//     identifier.
//   * emit_array_destructuring lowers each defaulted slot to
//     `(i < src.length && src[i] !== undefined) ? src[i] : default`.
//     The length guard short-circuits past the slot read on shorter
//     sources, sidestepping the typed-Array OOB-returns-garbage wedge
//     (handoff candidate #6); the `!== undefined` arm still fires for
//     the rare `Array<Nullable<T>>` slot-is-undefined case.

// Default fires when source shorter than the pattern.
const arr: number[] = [10];
const [a = 1, b = 2, c = 3] = arr;
console.log(a);   // 10
console.log(b);   // 2
console.log(c);   // 3

// All slots present — defaults inert.
const arr2: number[] = [10, 20, 30];
const [x = 0, y = 0, z = 0] = arr2;
console.log(x);   // 10
console.log(y);   // 20
console.log(z);   // 30

// Mixed: some slots defaulted, some not, no elision.
const arr3: number[] = [100];
const [p, q = 42] = arr3;
console.log(p);   // 100
console.log(q);   // 42

// String element type — non-numeric default expression.
const ss: string[] = ["hi"];
const [s1 = "x", s2 = "y"] = ss;
console.log(s1);  // hi
console.log(s2);  // y

// Default value referencing literal of different shape.
const [n1 = 7 + 1] = [] as number[];
console.log(n1);  // 8
