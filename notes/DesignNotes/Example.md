
```
let a = [14,25,32,41];
```

```
extern_functions = {
	alloc_gc_array,
	create_gc_frame
}

fun main:
	locals:
		L0:
		size : 1
		L1:
		size : 1
		

	load_imm L0, 4
	extern_call L1, alloc_array, L0
	
	
	load_imm L0, 0
	add L0, L1, L0
	load_imm L2, 14
	store_ind L1, L4
	
	load_imm L0, 0
	add L0, L1, L0
	load_imm L2, 14
	store_ind L1, L4
	
	 

```