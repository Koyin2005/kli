```

for c in chars("hello world\n") do 
	print_char(c)
end

type CharIter = ..
fun chars(s : string) -> (CharIter,fun(CharIter) -> Option[(char,CharIter)]) = ...



var run = true
var (state,next) = chars("hello world\n")
while run do 
	case next(state) of 
	| .Some((c,next_state)) -> do 
		do
			print_char(c)
		end;
		state = next_state;
	end
	| .None -> run = false
	end

end

```