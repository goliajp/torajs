package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	xs := make([]int64, 0, n)
	for i := int64(0); i < n; i++ {
		xs = append(xs, i)
	}
	var total int64 = 0
	for len(xs) > 0 {
		v := xs[len(xs)-1]
		xs = xs[:len(xs)-1]
		total += v
	}
	fmt.Println(total)
}
