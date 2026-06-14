// W-N-b — Object.getOwnPropertyNames(arr) returns ["0", ..., "<len-1>", "length"]
// Spec ES §22.1.3.5: Array's own properties enumerate as numeric-index
// strings + "length". 3 shapes: [10,20,30] / [] / [42]. Bun parity
// verified byte-equal. SSA-lower Arr arm Loads arr.len at ARR_LEN_OFF=8
// and routes through __torajs_arr_index_strs (torajs-meta::own_names).

console.log(Object.getOwnPropertyNames([10, 20, 30]));
console.log(Object.getOwnPropertyNames([]));
console.log(Object.getOwnPropertyNames([42]));
