// different: []string has no per-element type tag; main.ts reads an any[] and dispatches on the tag
package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	n, _ := strconv.ParseInt(os.Args[1], 10, 64)
	pool, _ := strconv.ParseInt(os.Args[2], 10, 64)
	// each element is a heap string, as 'item' + j is in main.ts
	var arr []string
	for j := int64(0); j < pool; j++ {
		arr = append(arr, "item"+strconv.FormatInt(j, 10))
	}
	var total int64 = 0
	for i := int64(0); i < n; i++ {
		s := arr[i%pool]
		total = total + int64(len(s))
	}
	fmt.Println(total)
}
