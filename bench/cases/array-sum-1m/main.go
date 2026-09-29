package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	count, _ := strconv.ParseInt(os.Args[1], 10, 64)
	xs := make([]int64, 0)
	for i := int64(0); i < count; i++ {
		xs = append(xs, i)
	}
	var sum int64 = 0
	for j := 0; j < len(xs); j++ {
		sum += xs[j]
	}
	fmt.Println(sum)
}
