package main

import (
	"fmt"
	"strings"
)

func rowLen(line string) int64 {
	var total int64 = 0
	for _, part := range strings.Split(line, ",") {
		total += int64(len(strings.TrimSpace(part)))
	}
	return total
}

func main() {
	var total int64 = 0
	var n int64 = 100_000
	for i := int64(0); i < n; i++ {
		total += rowLen("  alpha , beta , gamma , delta , epsilon , zeta  ")
	}
	fmt.Println(total)
}
