package main

import (
	"fmt"
	"strings"
)

func rebuild(line string) int64 {
	var total int64 = 0
	for _, part := range strings.Split(line, ",") {
		s := part + "|"
		total += int64(s[0])
	}
	return total
}

func main() {
	var total int64 = 0
	var n int64 = 100_000
	for i := int64(0); i < n; i++ {
		total += rebuild("alpha,beta,gamma,delta,epsilon,zeta")
	}
	fmt.Println(total)
}
