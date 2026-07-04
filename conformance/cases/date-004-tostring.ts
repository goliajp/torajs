// Date.prototype.toString — "Thu Jan 01 1970 09:00:00 GMT+0900
// (Japan Standard Time)" shape: local-time fields + TZif offset +
// CLDR long zone name (chunk 510 kernel). Typed and any tiers hit
// the same __torajs_date_to_string kernel.
const d = new Date(0);
console.log(d.toString());
const d2 = new Date(1720000000000);
console.log(d2.toString());
// negative-epoch (pre-1970) date
const d3 = new Date(-86400000);
console.log(d3.toString());
// any-tier receiver, same kernel through the Tag::Date arm
const a: any = new Date(0);
console.log(a.toString());
