// variadic fn-type annotations parse and register (RFC
// 20260708-variadic chunk 1: parser + checker Rest sentinel; the
// value lanes stay loud until the boxed_entry call lane lands)
type CB = (...args: any[]) => number;
type CB2 = (first: string, ...rest: number[]) => void;
console.log("ok");
