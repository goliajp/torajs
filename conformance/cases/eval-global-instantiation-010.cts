// A function a global eval declares is observable when the only
// thing that names it is `new f()`, so the eval is not folded away.
eval("function F(){ console.log('constructed') }");
new F();
eval("function P(a){ this.a = a }");
console.log(new P(4).a);
