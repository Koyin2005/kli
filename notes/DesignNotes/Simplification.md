
Should bounds checks be in the ir? 

On the Intermediate Representation:
This source code:
```
fun main() -> () = {
	let b = [[1,2,3,4],[5,6,7,8]]
	let c = true
	b.[0].[0] = if c { 23 } else { 32 }
	io.println(b.[0].[0])
	
}
```

becomes:
```
fun main() -> () = {
tmp0 : array[int]
tmp1 : array[int]
b : array[array[int]]
c : bool
tmp2 : array[int]
tmp3 : array[int]
tmp4 : array[int]
tmp5 : ()
bb0:
	tmp0 = MakeArray(1,2,3,4)
	tmp1 = MakeArray(5,6,7,8)
	b = MakeArray(tmp0,tmp1)
	c = true
	tmp2 = LoadIndex(b,0)
	if c then bb1 else bb2
bb1:
	StoreIndex(tmp2,0,23)
	goto bb3
bb2: 
	StoreIndex(tmp2,0,32)
	goto bb3
bb3:
	tmp3 = LoadIndex(b,0)
	tmp4 = LoadIndex(tmp3,0)
	tmp5 = io.println(tmp3,tmp4)
	return ()
}
```