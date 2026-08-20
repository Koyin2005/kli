
```
Source Code:
let a = Some(32)
case a of 
| .Some(x) -> println("The value of x is $(x)")
| .None -> println("Idk the value")

```


```
MIR:
bb0:
 a = Some(.0 = const 32)
 tmp0 = discriminant(a)
 switch tmp0 
  1 -> bb1
  0 -> bb2
bb1 :
  x = (a as Some).0
  tmp1 = fmt.new()
  tmp1 = fmt.append_string(tmp1,const "The value of x is ")
  tmp1 = fmt.append_value[int](tmp1,x)
  tmp2 = fmt.drain(tmp1)
  println(tmp2)
  goto bb3
bb2 :
  println(const "Idk the value")
  goto bb3
```



```
bb0:
	write [a], 1
	write [a + 4], 32
	
	write [tmp0] [a]
	switch [tmp0] 
	 1 -> bb1
	 2 -> bb2
bb1:
	write [x] [x + 4]
	...
	goto bb3
bb2:
	...
	goto bb3

```
