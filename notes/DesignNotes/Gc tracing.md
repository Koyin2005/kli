
So lets say we got a dynamic array how do we trace that

well
its a pointer to the "actual dyn array"

So a custom Gc object would be 
 a pair of trace function pointer vtable and a pointer to the gced data
essentially
```
We dont have pointers but if we did
Traced[T] = (fun(ptr[T]),ptr[T])

So 
fun make_dyn_array() = 
	(trace_dyn_array, boxed Header)
fun trace_dyn_array(this : ptr[Header]) -> () = 
	for value in this^.buf[0..this^.header.len]  trace(value)

```
