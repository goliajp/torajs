var p: any = Promise.resolve(1);
p.foo = 42;
console.log("h1", "foo" in p);
console.log("h2", "bar" in p);
console.log("d1", delete p.foo);
console.log("d2", p.foo);
p.foo2 = 7;
console.log("g1", JSON.stringify(Object.getOwnPropertyDescriptor(p, "foo2")));
console.log("g2", Object.getOwnPropertyDescriptor(p, "missing"));
