// S219 — Array.copyWithin(target [, start [, end ]]) per ES §23.1.3.4
// step 1/4/7: target/start ToIntegerOrInfinity(undef)=0,
// end===undefined → len.
console.log([1, 2, 3, 4].copyWithin(0, 2, undefined));
console.log([1, 2, 3, 4].copyWithin(0, undefined, 2));
console.log([1, 2, 3, 4].copyWithin(undefined, 2));
console.log([1, 2, 3, 4].copyWithin(undefined, undefined, undefined));
console.log([1, 2, 3, 4].copyWithin(1, undefined, undefined));
console.log([10, 20, 30, 40, 50].copyWithin(0, 3, undefined));
console.log(["a", "b", "c", "d"].copyWithin(0, 2, undefined));
console.log(["a", "b", "c", "d"].copyWithin(undefined, 2));
