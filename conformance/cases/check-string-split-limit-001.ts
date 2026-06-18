// ES §22.1.3.21 - String.prototype.split(separator, limit) — the
// 2-arg form was type-rejected ("expected 1 argument(s), got 2")
// even though ssa_lower_str.rs has had the V3-18 wedge code path
// for ages. check.rs's split sig declared only one Any param;
// widen to two and let the trailing-Any arity-pad keep the 1-arg
// path working.

console.log('a,b,c,d'.split(',', 2))
console.log('a,b,c,d'.split(',', 0))
console.log('a,b,c,d'.split(',', 10))
console.log('abc'.split('', 2))
console.log('hello'.split('l', 1))

// 1-arg form keeps working (regression check).
console.log('a,b,c,d'.split(','))
console.log('abc'.split(''))
