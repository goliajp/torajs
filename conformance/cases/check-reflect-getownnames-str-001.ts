// W-N-d — Object.getOwnPropertyNames(str) returns ["0", ..., "<len-1>", "length"]
// Spec ES §22.1.5.2.4: String's own enumerable properties are the index
// chars + the inherited-but-listed `length`. Same result shape as the
// W-N-b Arr arm; thin SSA-lower Type::Str arm delegates to a new
// torajs-meta::own_names wrapper that reads u32 len at STR_LEN_OFF=8
// then calls __torajs_arr_index_strs. 3 shapes: "hello" / "" / "x".
// Bun parity verified byte-equal.

console.log(Object.getOwnPropertyNames("hello"));
console.log(Object.getOwnPropertyNames(""));
console.log(Object.getOwnPropertyNames("x"));
