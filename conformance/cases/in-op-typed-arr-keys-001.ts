var arr = [1, 2, 3];
console.log("1" in arr);
console.log("q" in arr);
var i = 1;
console.log(i in arr);
console.log(5 in arr);
var s: any = "2";
console.log(s in arr);
var arr2, j;
arr2 = [1, 2, 3, 4, 5];
j = 1;
for (eval("j in arr2"); 1; ) { break; }
console.log("evalok");
