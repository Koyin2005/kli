Resizable
Heap allocated header plus heap buffer for sharing purposes

Needs some way of representing a "typed" raw buffer

```
type DynArray[T] = {
	inner : Box[Header[T]]
}
type Header[T] = {
	buf : RawBuf[T],
	len : uint
}

#Safety : element at `index` must be initialised
@unsafe
fun get(buf : RawBuf[T], index : uint) -> T;

@unsafe
fun set(buf : RawBuf[T], index : uint, value : T);

@unsafe
fun realloc(buf : RawBuf[T], new_size : uint) -> RawBuf[T];

RawArray should use checked indexing by default
```