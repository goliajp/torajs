// §14.11 — inside a with body a bare-name delete resolves through the
// scope object, so `eval("with(o){ delete p }")` in sloppy script code
// is a PROPERTY delete: the property really goes away and the delete
// answers true. (r443 regression: an early §13.5.1.2 fold in the eval
// inline constant-folded the site and deleted nothing.)
this.p1 = 1;
var myObj = { p1: "a", del: false };
eval("with(myObj){del = delete p1}");
console.log("w1", myObj.p1 === undefined, myObj.del === true);
