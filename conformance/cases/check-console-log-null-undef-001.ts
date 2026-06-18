// S139 — `console.log(null)` and `console.log(undefined)` print
// 'null' / 'undefined' (per Node util.inspect), not the underlying
// pointer sentinel "0". Both lower to ConstPtrNull at the operand
// layer; the dispatch uses frontend Type::Null / Type::Undefined
// (expr_types) as the source of truth. Covers literal forms,
// derived S138-style expressions, and multi-arg `undefined`.
console.log(null);
console.log(undefined);
console.log(null, null);
console.log(undefined, undefined);
console.log(null, undefined);
console.log(undefined, null);
console.log(null, 'x', undefined);
console.log(null && 'x');         // S138 derived → null
console.log(undefined || 'd');    // S138 derived → 'd'
console.log(undefined, null && 'x');
