package main

import (
	"fmt"
	"os"
	"strconv"
)

func main() {
	count, _ := strconv.ParseInt(os.Args[1], 10, 64)
	src := make([]int64, 0)
	for i := int64(0); i < count; i++ {
		src = append(src, i)
	}
	dst := make([]int64, 0)
	for i := 0; i < len(src); i++ {
		dst = append(dst, src[i])
	}
	fmt.Println(int64(len(dst)) + dst[count-1])
}
