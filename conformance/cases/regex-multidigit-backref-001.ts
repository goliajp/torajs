// chunk 802 — ES DecimalEscape reads the longest digit run: \12
// references group 12 (the old parser read one digit — \1 then a
// literal '2', a silent mismatch on every \10+ backref).
console.log(/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)(l)\12\11/.test("abcdefghijkllk"));
console.log("abcdefghijkllk".match(/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)(l)\12/)[0]);
const many = /(x)(y)(z)(x)(y)(z)(x)(y)(z)(x)(y)(z)(x)(y)(z)(x)(y)(z)(x)(y)(z)(x)(y)(z)(x)(y)(z)(x)(y)(z)(x)(y)(z)\33/;
console.log(many.test("xyzxyzxyzxyzxyzxyzxyzxyzxyzxyzxyzz"));
console.log(many.test("xyzxyzxyzxyzxyzxyzxyzxyzxyzxyzxyzq"));
console.log(/(q)\1/.test("qq"));
// NOTE: split() splicing capture-group values into the result array
// (ES §22.1.3.21 step 14.c.iii) is a separate pre-existing gap.
