
```
let a = [1,2,3,4]
a.[0] = 5;
```

```
bb0:
 a = AllocArray(1,2,3,4)
 index = 0
 l = Len(a)
 tmp0 = index <  l
 assert tmp0, in_bounds -> bb1
bb1:
 a.[index] = 5
 return
 
```

```
bb0:
	call a, gc_alloc(16)
	move [a] + 4, 4
	move index, 0
	move l, [a + 4]
	lt tmp0, [index], [l]
	brif [tmp0] 
	 true -> bb1
	 false -> out_of_bounds_0
bb1:
	mul %0, [index], 4
	add %0, %0, [a]
	move [%0], 5 
	return
out_of_bounds_0:
	call bounds_check_failed
	trap


```