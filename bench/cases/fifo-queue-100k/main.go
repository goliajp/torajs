package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	q := make([]int64, 0, 32)
	var total int64 = 0
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	window, _ := strconv.ParseInt(os.Args[2], 10, 64)
	for i := int64(0); i < n; i++ {
		q = append(q, i)
		if int64(len(q)) > window {
			total += q[0]
			q = q[1:]
		}
	}
	for len(q) > 0 {
		total += q[0]
		q = q[1:]
	}
	fmt.Println(total)
}
