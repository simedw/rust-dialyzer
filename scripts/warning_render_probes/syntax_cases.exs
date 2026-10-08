{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer),[:abstract_code])
forms=Enum.map(forms,fn {:attribute,a,:module,_}->{:attribute,a,:module,:syntax_cases_native};f->f end)
{:ok,:syntax_cases_native,binary,_}=:compile.forms(forms,[:binary,:export_all,:return_errors,:return_warnings])
{:module,:syntax_cases_native}=:code.load_binary(:syntax_cases_native,~c"syntax_cases_native",binary)
base=["integer()","atom()","none()","fun()","fun((...) -> atom())","fun((A, B) -> A | B)","1","-1","+2","16#FF","$\\n","$\\s","$\\x{3BB}","'hello'","'maybe'","'λ'","'\\x{1F600}'","A","_", "[]","<<>>","<<_:7>>","<<_:_*8>>","<<_:3,_:_*4>>","1..255","A :: atom()","m:t(A)","#r{a :: atom(), b :: integer()}","\#{atom() => integer(), 'k' := A}","[atom()]","[atom(),...]","maybe_improper_list(atom(), [])"]
composed=for {a,i}<-Enum.with_index(base),{b,j}<-Enum.with_index(base), rem(i*7+j,11)==0, shape<-0..2 do
 case shape do 0->"{#{a},#{b}}";1->"#{a} | #{b}";2->"(#{a},#{b})" end
end
literal=["3.14","2.0","1.0e20","1.0e-20","\"a\\n\\t\\001\\177λ\"","\""<>String.duplicate("a",200)<>"\"","[1,2|Tail]","[H = {'target', _, _} | T]","\#{a => 1, b := 2}","<<1,2>>","<<1:3, B/binary-unit:8>>","<<\"abc\">>","A + B * C","1 bor 2","not true","A == B","A =:= B","A andalso B","A orelse B","{A = 1, B = 2}","(a,b)","<integer(), atom()>","(1+2)","<<_:1+2>>","<<_:_*2*4>>"]
invalid=["", "(","[", "<<", "a b", "{'a'", "#r{a ::}", "A ::", "1e2", "a(,)","λ","字","A\u2003", "'"<>String.duplicate("a",256)<>"'", "<<_:3,_:4>>", "<<_:_*8,_:3>>"]
rows=(for source<-base++composed++literal++invalid,do: {"t",source})++(for source<-["() -> atom()","(...) -> atom()","(A,B) -> A | B when A :: atom(), B :: integer()","(atom()) -> atom(); (integer()) -> integer()","(A) -> A when is_subtype(A, atom())","integer()", "(A) -> A when thing(A, atom())"],do: {"sig",source})++(for source<-["x::atom()", "x :: integer() and y::A | B","x","x::::atom()"],do: {"field_diffs",source})
for {{helper,source},id}<-Enum.with_index(rows) do
 result=try do
  expected=apply(:syntax_cases_native,String.to_atom(helper),[String.to_charlist(source),true])|>:lists.flatten()|>List.to_string()
  %{expected: expected}
 catch kind,reason->%{expected_error: %{class: Atom.to_string(kind),reason: inspect(reason)}} end
 IO.puts(:json.encode(Map.merge(%{id: id,helper: helper,source: source},result)))
end
