var r: string[] = [];
r.push(String(JSON.parse("1")));
r.push(String(JSON.parse("-2.5e2")));
r.push(String(JSON.parse("\"hi\"")));
r.push(String(JSON.parse("true")));
r.push(String(JSON.parse("null")));
r.push(String((JSON.parse("[1,[2,3],{\"k\":4}]") as any)[2].k));
r.push(String((JSON.parse("  {\"a\": {\"b\": [null, false]}}  ") as any).a.b[1]));
try { JSON.parse("{bad"); r.push("nothrow"); } catch (e) { r.push("syn1"); }
try { JSON.parse("1 2"); r.push("nothrow"); } catch (e) { r.push("syn2"); }
try { JSON.parse(""); r.push("nothrow"); } catch (e) { r.push("syn3"); }
var p: any = JSON.parse("{\"__proto__\": {\"x\": 9}}");
r.push(String(p.__proto__.x), String(p.x));
r.push(String(JSON.parse(123 as any)));
console.log(r.join(" "));
