// chunk 803 — split() splices capture-group values into the result
// array after each separator match (ES §22.1.3.21 step 14.c.iii):
// participating groups as their text, non-participating as undefined.
console.log("aXbXc".split(/(X)/));
console.log("aXbXc".split(/(X)/).length);
console.log("a1b2c".split(/(\d)/).join("-"));
console.log("axbyc".split(/(x)|(y)/));
console.log("axbyc".split(/(x)|(y)/).length);
console.log("hello world".split(/(\s)/));
console.log("nosep".split(/(q)/));
console.log("aXb".split(/X/));
console.log("a,b,,c".split(/(,)/, 3));
