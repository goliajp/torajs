// S206 — `Array.prototype.join(sep?)` routes explicit `undefined`
// to the default-comma path per ES §23.1.3.16 step 1: if sep is
// undefined → sep = ",".
//
// Pre-S206 tr rejected the 1-arg-undefined form at the strict-arity
// gate ("argument 0: expected String, got Undefined") because the
// declared `vec![Type::String]` signature didn't tolerate the
// Undefined-typed arg. The 0-arg form was already covered by
// V3-18 m1.h.42.

// String, Number, Boolean, Any element arrays — each helper that
// V3-18 m1.h.43 dispatches must accept the undefined sep.
console.log('str:', ['a', 'b', 'c'].join(undefined));
console.log('num:', [1, 2, 3].join(undefined));
console.log('bool:', [true, false, true].join(undefined));

// 0-arg regression hold.
console.log('str-noarg:', ['a', 'b', 'c'].join());
console.log('num-noarg:', [1, 2, 3].join());

// Custom sep still applied per spec.
console.log('str-sep:', ['a', 'b', 'c'].join('-'));
console.log('num-sep:', [1, 2, 3].join(' / '));

// Side-effect ordering — receiver evaluated even when the
// argument is the literal `undefined` (parity with bun, which
// still walks the receiver binding before dispatching).
let n = 0;
const make = (): number[] => { n++; return [1, 2, 3]; };
console.log('side-effect:', make().join(undefined));
console.log('side-effect-count:', n);

// Any-element array — the runtime path picks `arr_join_any`;
// confirm the helper still gets the default sep through the
// expr_types lookup.
const anyArr: any[] = [1, 'b', true];
console.log('any:', anyArr.join(undefined));
