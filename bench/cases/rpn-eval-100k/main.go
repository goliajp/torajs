package main

import (
	"fmt"
	"strings"
)

func evalRpn(expr string) int64 {
	var stack [16]int64
	sp := 0
	for _, tok := range strings.Split(expr, " ") {
		c0 := int64(tok[0])
		if c0 >= 48 && c0 <= 57 {
			stack[sp] = c0 - 48
			sp++
		} else {
			b := stack[sp-1]
			a := stack[sp-2]
			sp -= 2
			var r int64
			if c0 == 43 {
				r = a + b
			} else if c0 == 45 {
				r = a - b
			} else {
				r = a * b
			}
			stack[sp] = r
			sp++
		}
	}
	return stack[0]
}

func main() {
	var total int64 = 0
	var n int64 = 100_000
	for i := int64(0); i < n; i++ {
		total += evalRpn("3 4 + 2 * 5 +")
	}
	fmt.Println(total)
}
