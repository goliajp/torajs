console.log(JSON.stringify(Object.values({a:1,b:"x"})));
console.log(JSON.stringify(Object.values({a:1,b:"x",c:true,d:null})));
console.log(JSON.stringify(Object.values({})));
console.log(JSON.stringify(Object.values({a:1,b:2})));
console.log(JSON.stringify(Object.values({s:"p",t:"q"})));
const v = Object.values({n:5,s:"z"});
console.log(v.length, v[0], v[1], typeof v[0], typeof v[1]);
console.log(JSON.stringify(Object.values({a:1,b:[2,3]})));
console.log(JSON.stringify(Object.values({a:{x:1},b:"y"})));
