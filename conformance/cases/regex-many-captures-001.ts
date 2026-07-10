// chunk 801 — the 32-capture-group cap became a 65536 sanity bound
// (V8 kMaxCaptures): save rows are stride-sized per program, so
// group counts past the old fixed-buffer boundary match, exec,
// backreference and $NN-replace like any other pattern.
const re40 = /(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)(l)(m)(n)(o)(p)(q)(r)(s)(t)(u)(v)(w)(x)(y)(z)(0)(1)(2)(3)(4)(5)(6)(7)(8)(9)(A)(B)(C)(D)/;
const hay = "abcdefghijklmnopqrstuvwxyz0123456789ABCD";
const m = hay.match(re40);
console.log(m.length);
console.log(m[1]);
console.log(m[35]);
console.log(m[40]);
const ex = re40.exec(hay);
console.log(ex.length);
console.log(ex[38]);
console.log(ex.index);
console.log(hay.replace(re40, "$40-$35-$1"));
console.log("xx" + hay.replace(re40, "$12$11$10"));
// NOTE: a multi-digit backref (\NN) into the new group range is a
// separate pre-existing parser gap (decimal backrefs read a single
// digit) — chunk 802 scope.
console.log("abc".match(/(a)(b)/).length);
