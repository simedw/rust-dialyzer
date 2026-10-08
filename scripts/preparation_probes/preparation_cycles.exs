Code.require_file("../core_export.exs",__DIR__)
[env,dir]=System.argv();File.mkdir_p!(dir)
specs=[{"pair","f(A,B) -> {A,B} when A :: [B], B :: {A}"},{"pair_reverse","f(B,A) -> {B,A} when B :: {A}, A :: [B]"},{"triangle","f(A,B,C) -> {A,B,C} when A :: [B], B :: {C}, C :: [A]"},{"self","f(A) -> A when A :: [A]"},{"acyclic","f(A,B) -> {A,B} when A :: [B], B :: integer()"}]
pairs=[{"de","D","E"},{"alpha","Alpha","Beta"},{"za","Z","A"},{"xy","X","Y"},{"things","Thing","Other"},{"sides","Left","Right"},{"long","VarOne","VarTwo"},{"underscore","_A","_B"},{"kv","K","V"}]
specs=specs++[{"self_and_long","f(A,B) -> {A,B} when A :: {A,B}, B :: {A}"},{"self_and_long_reverse","f(A,B) -> {A,B} when A :: {B,A}, B :: {A}"},{"diamond","f(A,B,C) -> {A,B,C} when A :: {B,C}, B :: {A}, C :: [A]"},{"diamond_reverse","f(A,B,C) -> {A,B,C} when A :: {C,B}, C :: [A], B :: {A}"},{"overlapping","f(A,B,C,D) -> {A,B,C,D} when A :: {B,C}, B :: {D}, C :: [D], D :: {A}"}]++(for {id,a,b}<-pairs,do: {id,"f(#{a},#{b}) -> {#{a},#{b}} when #{a} :: [#{b}], #{b} :: {#{a}}"})
rows=for {id,spec}<-specs do
 name="bounded_cycle_"<>id;arity=cond do id in ["triangle","diamond","diamond_reverse"]->3;id=="overlapping"->4;id=="self"->1;true->2 end
 args=Enum.map_join(1..arity,",",fn n->"A#{n}"end);source=Path.join(dir,name<>".erl")
 File.write!(source,"-module(#{name}).\n-export([f/#{arity}]).\n-spec #{spec}.\nf(#{args})->{#{args}}.\n")
 case :compile.file(String.to_charlist(source),[:debug_info,:return_errors,:return_warnings,{:outdir,String.to_charlist(dir)}]) do
  {:ok,m,warnings}->
   IO.inspect({name,:compiled,warnings})
   {:ok,{^m,[debug_info: {:debug_info_v1,backend,metadata},abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(String.to_charlist(Path.join(dir,name<>".beam")),[:debug_info,:abstract_code])
   {:ok,unclean}=backend.debug_info(:core_v1,m,metadata,[:no_copt,:to_core,:binary,:return_errors,:no_inline]);c=:dialyzer_clean_core.clean(unclean)
   path=Path.join(dir,name<>".core.json");File.write!(path,:json.encode(%{version: 6,profile: CoreExport.profile(),modules: [CoreExport.export_module(m,forms,c)]}));%{id: name,core: Path.expand(path),environment: Path.expand(env)}
  result->IO.inspect({name,:rejected,result});nil
 end
end|>Enum.reject(&is_nil/1)
# Exhaust all three-vertex directed graphs in one real compiler module; alternate
# RHS/source constraint ordering to exercise digraph bag/DFS traversal.
name="bounded_cycle_graphs"; source=Path.join(dir,name<>".erl")
functions=for mask<-0..511 do
 vars=["A","B","C"];n="f#{mask}"
 constraints=for {v,i}<-Enum.with_index(vars) do
  uses=for {u,j}<-Enum.with_index(vars),:erlang.band(mask,:erlang.bsl(1,3*i+j))!=0,do: u
  uses=if rem(mask,2)==0,do: uses,else: Enum.reverse(uses)
  rhs=if uses==[],do: "integer()",else: "{"<>Enum.join(uses,",")<>"}"
  v<>" :: "<>rhs
 end
 constraints=if rem(mask,3)==0,do: Enum.reverse(constraints),else: constraints
 "-spec #{n}(A,B,C) -> {A,B,C} when "<>Enum.join(constraints,",")<>".\n#{n}(A,B,C)->{A,B,C}.\n"
end
File.write!(source,"-module(#{name}).\n-export(["<>Enum.map_join(0..511,",",fn n->"f#{n}/3"end)<> "]).\n"<>Enum.join(functions))
{:ok,m,[]}=:compile.file(String.to_charlist(source),[:debug_info,:return_errors,:return_warnings,{:outdir,String.to_charlist(dir)}])
{:ok,{^m,[debug_info: {:debug_info_v1,backend,metadata},abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(String.to_charlist(Path.join(dir,name<>".beam")),[:debug_info,:abstract_code])
{:ok,unclean}=backend.debug_info(:core_v1,m,metadata,[:no_copt,:to_core,:binary,:return_errors,:no_inline]);c=:dialyzer_clean_core.clean(unclean)
path=Path.join(dir,name<>".core.json");File.write!(path,:json.encode(%{version: 6,profile: CoreExport.profile(),modules: [CoreExport.export_module(m,forms,c)]}))
rows=rows++[%{id: name,core: Path.expand(path),environment: Path.expand(env)}]
File.write!(Path.join(dir,"requests.jsonl"),Enum.map(rows,fn r->[:json.encode(r),"\n"]end))
