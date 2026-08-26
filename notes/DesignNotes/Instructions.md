
```

pub enum Instr{
	LoadInd{
		dst : Local,
		src : Local
	},
	StoreInd{
		dst : Local,
		src : Local,
	},
	Move{
		dst : Local,
		src : Local	
	},
	LoadImm {
		dst : Local,
		src : i64
	},
	Call {
		dst : Local,
		function : FuncId,
		arg_start : Local,
		arg_count : u32
	},
	ExternCall {
		dst : Local,
		function : ExternFuncId,
		arg_start : Local,
	},
	Add {
		dst : Local,
		src1 : Local,
		src2 : Local 
	},
	Sub {
		dst : Local,
		src1 : Local,
		src2 : Local 
	},
	Div {
		dst : Local,
		src1 : Local,
		src2 : Local 
	},
	Mul {
		dst : Local,
		src1 : Local,
		src2 : Local 
	},
	
}
```