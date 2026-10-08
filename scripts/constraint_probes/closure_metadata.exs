[root,out]=System.argv()
Code.require_file(Path.join(root,"scripts/type_codec.exs"))
Code.require_file(Path.join(root,"scripts/core_export.exs"))
path=:code.which(:dialyzer_typesig)
{:ok,:dialyzer_typesig,chunks}=:beam_lib.all_chunks(path)
get=fn name->chunks|>List.keyfind(String.to_charlist(name),0)|>elem(1)end
{:ok,{:dialyzer_typesig,[{:atoms,atoms}]}}=:beam_lib.chunks(path,[:atoms])
atoms=Map.new(atoms)
<<fun_count::32,fun_entries::binary>>=get.("FunT")
fun_table=for <<name::32,arity::32,label::32,index::32,num_free::32,uniq::32<-fun_entries>>,do: %{name: Atom.to_string(Map.fetch!(atoms,name)),arity: arity,label: label,index: index,env_count: num_free,uniq: uniq}
true=length(fun_table)==fun_count
fun_table=Enum.with_index(fun_table)|>Enum.map(fn {entry,new_index}->Map.put(entry,:new_index,new_index)end)
read_entries=fn <<count::32,rest::binary>>->entries=for <<a::32,arity::32,label::32<-rest>>,do: {a,arity,label};true=count==length(entries);entries end
exports=read_entries.(get.("ExpT"));locals=read_entries.(get.("LocT"))
entries=Enum.uniq(exports++locals)|>Enum.sort()
new_exp=<<length(entries)::32>><>IO.iodata_to_binary(Enum.map(entries,fn {a,b,c}-><<a::32,b::32,c::32>>end))
{:ok,patched}=:beam_lib.build_module(List.keyreplace(chunks,~c"ExpT",0,{~c"ExpT",new_exp}))
{:ok,:dialyzer_typesig,patched_chunks}=:beam_lib.all_chunks(patched)
true=Enum.all?(chunks,fn {name,data}->name==~c"ExpT" or List.keyfind(patched_chunks,name,0)=={name,data}end)
:code.purge(:dialyzer_typesig)
{:module,:dialyzer_typesig}=:code.load_binary(:dialyzer_typesig,path,patched)
# Build only the default record value from the pinned source layout in a disjoint helper.
source=Path.join(root,".cache/upstream/otp/lib/dialyzer/src/dialyzer_typesig.erl")
helper_path=Path.join(__DIR__,"native_typesig_metadata_probe.erl")
helper_source=File.read!(source)|>String.replace("-module(dialyzer_typesig).","-module(native_typesig_metadata_probe).\n-export([default_state/0]).")
File.write!(helper_path,helper_source<>"\ndefault_state()->#state{}.\n")
{:ok,:native_typesig_metadata_probe,helper}=:compile.file(String.to_charlist(helper_path),[:binary,{:i,String.to_charlist(Path.dirname(source))}])
{:module,:native_typesig_metadata_probe}=:code.load_binary(:native_typesig_metadata_probe,String.to_charlist(helper_path),helper)
state=:native_typesig_metadata_probe.default_state()

walk=fn recur,term,acc->case term do
 {:fun_var,fun,deps,origin}-> info=Map.new(:erlang.fun_info(fun));[%{module: Atom.to_string(info.module),name: Atom.to_string(info.name),env_terms: CoreExport.export_terms(info.env),new_index: info.new_index,index: info.index,uniq: info.uniq,arity: info.arity,env: inspect(info.env,limit: :infinity),env_count: length(info.env),deps: deps,origin: origin}|acc]
 {:constraint,lhs,relation,rhs,deps}->Enum.map(recur.(recur,rhs,[]),fn item->Map.merge(item,%{lhs: CoreExport.export_terms(lhs),relation: Atom.to_string(relation),constraint_deps: deps})end)++acc
 t when is_tuple(t)-> Enum.reduce(Tuple.to_list(t),acc,fn child,a->recur.(recur,child,a)end)
 l when is_list(l)->Enum.reduce(l,acc,fn child,a->recur.(recur,child,a)end)
 _->acc
 end end
mf=[{:+,2},{:-,2},{:*,2},{:<,2},{:"=<",2},{:>,2},{:>=,2},{:++,2},{:and,2},{:or,2},{:not,1},{:"=:=",2},{:==,2},{:is_integer,1},{:is_function,2},{:is_record,2},{:is_record,3},{:element,2},{:get_module_info,1}]
collect=fn recur,term,acc->case term do
 {:fun_var,fun,_,_}->[fun|acc]
 t when is_tuple(t)->Enum.reduce(Tuple.to_list(t),acc,fn child,a->recur.(recur,child,a)end)
 l when is_list(l)->Enum.reduce(l,acc,fn child,a->recur.(recur,child,a)end)
 _->acc
 end end
terms=Enum.map(mf,fn {f,arity}->args=Enum.map(1..arity,&:erl_types.t_var/1);{{f,arity},:dialyzer_typesig.get_bif_constr({:erlang,f,arity},:erl_types.t_var(100),args,state)}end)
v=fn label->{:c_var,[{:label,label}],String.to_atom("v#{label}")}end
lit=fn x->{:c_literal,[],x}end
cons={:c_cons,[{:label,100}],v.(1),v.(2)}
segment={:c_bitstr,[{:label,101}],v.(1),v.(2),lit.(8),lit.(:integer),lit.([:unsigned,:big])}
binary={:c_binary,[{:label,100}],[segment]}
map={:c_map,[{:label,100}],v.(1),[{:c_map_pair,[],lit.(:assoc),v.(2),v.(3)},{:c_map_pair,[],lit.(:exact),v.(4),v.(5)}],false}
structural=Enum.map([{:cons,cons,state},{:binary,binary,state},{:binary_match_integer,binary,put_elem(state,7,true)},{:binary_construct_binary,{:c_binary,[{:label,100}],[put_elem(segment,5,lit.(:binary))]},state},{:binary_match_binary,{:c_binary,[{:label,100}],[put_elem(segment,5,lit.(:binary))]},put_elem(state,7,true)},{:map,map,state}],fn {kind,tree,context}->defined=:sets.from_list([1,2,3,4,5]);{new_state,_}=:dialyzer_typesig.traverse(tree,defined,context);{{kind,0},elem(new_state,3)}end)
terms=terms++structural
result=Enum.map(terms,fn {{f,arity},cs}->%{function: Atom.to_string(f),arity: arity,closures: Enum.reverse(walk.(walk,cs,[]))}end)
functions=Enum.flat_map(terms,fn {_,cs}->Enum.reverse(collect.(collect,cs,[]))end)++[&:erlang.length/1,&:lists.map/2]
extra_capture_values=[1,1.0,0,-0.0,0.0,9_007_199_254_740_993,9_007_199_254_740_992.0,:a,:é,[],[1|:tail],{1,1.0},%{1=>1.0,1.0=>1},<<1,2>>,<<3::3>>,%{}]
extra_capture_values=Enum.drop(extra_capture_values,-1)
functions=functions++Enum.map(extra_capture_values,fn captured->fn()->captured end end)
metadata=fn fun->info=Map.new(:erlang.fun_info(fun));if info.type==:external do
 %{kind: "external",module: Atom.to_string(info.module),function: Atom.to_string(info.name),arity: info.arity}
 else
 %{kind: "local",module: Atom.to_string(info.module),name: Atom.to_string(info.name),new_index: info.new_index,index: info.index,uniq: info.uniq,arity: info.arity,env_terms: CoreExport.export_terms(info.env),env_count: length(info.env)}
 end end
pairs=for a<-functions,b<-functions,do: %{a: metadata.(a),b: metadata.(b),native_order: cond do a<b->-1;a>b->1;true->0 end}
File.write!(out,:json.encode(%{module: "dialyzer_typesig",beam: List.to_string(path),exp_t_only_patch: true,original_sha256: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(path)))),fun_table: fun_table,roles: result,pairs: pairs}))
