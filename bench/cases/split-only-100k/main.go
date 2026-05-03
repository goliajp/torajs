package main

import (
	"fmt"
	"strings"
)

func main() {
	var total int64 = 0
	var n int64 = 100_000
	for i := int64(0); i < n; i++ {
		parts := strings.Split("3 4 + 2 * 5 +", " ")
		total += int64(len(parts))
	}
	fmt.Println(total)
}
