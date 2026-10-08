Code.require_file("../core_export.exs",__DIR__)
{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer),[:abstract_code])
{:function,_,:message_to_string,3,clauses}=Enum.find(forms,&match?({:function,_,:message_to_string,3,_},&1))
defmodule Synthetic do
 def value({:atom,_,a},_),do: a
 def value({:integer,_,n},_),do: n
 def value({:nil,_},_),do: []
 def value({:cons,_,h,t},kind),do: [value(h,kind)|value(t,kind)]
 def value({:tuple,_,items},kind),do: items|>Enum.map(&value(&1,kind))|>List.to_tuple()
 def value({:var,_,name},kind),do: variable(name,kind)
 def variable(name,_) when name in [:M,:B],do: :"module-name"
 def variable(:F,_),do: :"fun-name"
 def variable(name,_) when name in [:A,:Arity],do: 2
 def variable(:N,_),do: 22
 def variable(:Name,:no_return),do: [:"fun-name",2]
 def variable(:Name,_),do: :rec
 def variable(:Type,:no_return),do: :only_normal
 def variable(:Op,:fun_app_no_fun),do: ~c"Fun"
 def variable(name,_) when name in [:Op,:Infix],do: :"=:="
 def variable(:Guard,_),do: :is_atom
 def variable(:Fun,_),do: ~c"is_atom"
 def variable(:FailReason,_),do: :only_sig
 def variable(name,_) when name in [:Args,:SigArgs,:ArgStrings],do: ~c"(integer(), atom())"
 def variable(:ArgNs,_),do: [1,2,11,22]
 def variable(:Contract,kind) when kind in [:call,:apply],do: {false,~c"(integer(), atom()) -> integer()"}
 def variable(name,_) when name in [:Contract,:Sig],do: ~c"(integer(), atom()) -> integer()"
 def variable(:InvalidContractDetails,_),do: {[1,2],true}
 def variable(:Location,_),do: {20,5}
 def variable(:Conflicts,_) do
   type=:erl_types.t_nominal({:owner,:opaque,0,:opaque},:erl_types.t_integer())
   [{1,type,~c"owner:opaque()"}]
 end
 def variable(:ExpectedTypes,_),do: [:erl_types.t_integer()]
 def variable(name,_) when name in [:Pat,:Field],do: ~c"pattern {'ok', X}"
 def variable(:FieldDiffs,_),do: ~c"field :: integer() and other :: atom()"
 def variable(:RecConstr,_),do: ~c"#rec{field :: integer()}"
 def variable(:String,_),do: ~c"record #rec{}"
 def variable(:IsOpaque,_),do: true
 def variable(:Culprit,_),do: ~c"size"
 def variable(name,_) when name in [:Size,:Seg],do: ~c"<<V:8>>"
 def variable(:Arg,_),do: ~c"X"
 def variable(_, _),do: ~c"integer()"
end
Code.ensure_loaded!(:erl_types)
defmodule Branches do
 def variants({kind,args}) when kind in [:call,:apply] do
   {reason_index,positions_index,contract_index}=if kind==:call,do: {4,3,7},else: {2,1,5}
   for reason<-[:only_sig,:only_contract,:both],positions<-[[],[1],[1,2,3,11,21,22,23,24]],overloaded<-[false,true] do
     {kind,args|>List.replace_at(reason_index,reason)|>List.replace_at(positions_index,positions)|>List.replace_at(contract_index,{overloaded,~c"(integer(), atom()) -> integer()"})}
   end
 end
 def variants({:no_return,_}) do
   for reason<-[:no_match,:only_explicit,:only_normal,:both],name<-[[],[:"fun-name",2]],do: {:no_return,[reason|name]}
 end
 def variants({:opaque_compare,args}) do
   for op<-[:"=:=",:"==",:"=/=",:"/="],do: {:opaque_compare,List.replace_at(args,1,op)}
 end
 def variants({:opaque_union,args}),do: for(opaque<-[false,true],do: {:opaque_union,List.replace_at(args,0,opaque)})
 def variants({:invalid_contract,[m,f,a,details,contract,sig]}) when details != :none do
   for indices<-[[],[1],[1,2,3,11,21,22,23,24]],range<-[false,true],indices != [] or range,do: {:invalid_contract,[m,f,a,{indices,range},contract,sig]}
 end
 def variants(message),do: [message]
end
for {clause,index}<-Enum.with_index(clauses) do
 {:clause,_,[{:tuple,_,[{:atom,_,kind},_]}=pattern,_,_],_,_}=clause
 message=Synthetic.value(pattern,kind)
 for {message,variant}<-Enum.with_index(Branches.variants(message)),indent<-[false,true],filename<-[:basename,:fullpath],location<-[:line,:column] do
   warning={:warn_matching,{~c"/tmp/path/example.erl",{12,3},{:m,:f,2}},message}
   text=:dialyzer.format_warning(warning,[indent_opt: indent,filename_opt: filename,error_location: location])|>List.to_string()
   IO.puts(:json.encode(%{id: "source/"<>Integer.to_string(index)<>"/"<>Integer.to_string(variant),options: %{indent: indent,filename: Atom.to_string(filename),location: Atom.to_string(location)},warning: CoreExport.export_terms(warning),expected: text}))
 end
end

for atom<-[:plain,:maybe,:else,:"function-name",:"雪",:"é",:"emoji😀"],file<-[~c"",~c"/",~c"/tmp/../file.erl/",~c"雪/file.erl",~c"."],location<-[0,-2,7,{7,0},{7,3}],filename<-[:basename,:fullpath],error_location<-[:line,:column] do
 warning={:warn_callgraph,{file,location},{:call_to_missing,[atom,atom,2]}}
 opts=[indent_opt: false,filename_opt: filename,error_location: error_location]
 IO.puts(:json.encode(%{id: "scalar/"<>Atom.to_string(atom),options: %{indent: false,filename: Atom.to_string(filename),location: Atom.to_string(error_location)},warning: CoreExport.export_terms(warning),expected: :dialyzer.format_warning(warning,opts)|>List.to_string()}))
end
