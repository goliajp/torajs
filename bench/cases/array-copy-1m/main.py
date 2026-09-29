import sys

count = int(sys.argv[1])
src = []
i = 0
while i < count:
    src.append(i)
    i += 1
dst = []
j = 0
while j < len(src):
    dst.append(src[j])
    j += 1
print(len(dst) + dst[count - 1])
