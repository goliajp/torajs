// §25.5.2 — the any-lane JSON serializer's Str arm must materialize
// a Substr view (split-product slot) before quoting: the raw cell
// printed its own view-struct fields as garbage characters.
const x: any = "l";
console.log(JSON.stringify("hello".split(x)));
const z: any = /l/;
console.log(JSON.stringify("hello".split(z)));
const cm: any = ",";
console.log(JSON.stringify("a,b,c".split(cm, 2)));
const s2: any = new String("hello");
console.log(JSON.stringify(s2.split("l")));
const c: any = "hello";
console.log(JSON.stringify(c.split("l")));
const b: any = new String("hello-this-is-a-very-long-string-hello");
console.log(JSON.stringify(b.split("l")));
