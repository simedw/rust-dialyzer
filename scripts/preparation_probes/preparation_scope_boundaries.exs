Code.require_file("../core_export.exs",__DIR__)
[base,dir]=System.argv();File.mkdir_p!(dir)
original=base|>File.read!()|>:json.decode()
tables=CoreExport.rebuild_terms(original["type_tables"]);exports=CoreExport.rebuild_terms(original["exported_types"])
decl=fn m,body,params,file->{{m,{file,1},body,params},:any}end
local={:user_type,7,:leaf,[]};record={:type,7,:record,[{:atom,7,:r}]};unknown={:remote_type,7,[{:atom,7,:missing_redirect},{:atom,7,:rt},[]]}
cases=[{"redirect_scalar",:type,{:type,7,:integer,[]},[],0},{"redirect_local",:type,local,[],0},{"redirect_record",:type,record,[],0},{"redirect_opaque",:opaque,local,[],0},{"redirect_nominal",:nominal,record,[],0},{"redirect_event",:type,unknown,[],0},{"integer_parameter",:type,{:var,7,77},[77],1},{"duplicate_parameter",:type,{:var,7,:A},[:A,:A],2},{"underscore_parameter",:type,{:var,7,:_},[:_],1},{"tag_precedence",:type,{:type,7,:integer,[]},[],0}]
rows=for {id,tag,body,params,arity}<-cases do
 a=%{{tag,:rt,arity}=>decl.(:redirect_definition,body,params,~c"redirect_source.erl"),{:type,:leaf,0}=>decl.(:scope_lookup,{:type,7,:integer,[]},[],~c"lookup.erl"),{:record,:r}=>{{~c"lookup.erl",1},[{1,[{:x,{:type,7,:integer,[]},:erl_types.t_integer()}]}]}}
 a=if id=="tag_precedence",do: a|>Map.put({:opaque,:rt,0},decl.(:redirect_definition,{:type,7,:atom,[]},[],~c"opaque.erl"))|>Map.put({:nominal,:rt,0},decl.(:redirect_definition,{:type,7,:float,[]},[],~c"nominal.erl")),else: a
 b=%{{:type,:leaf,0}=>decl.(:redirect_definition,{:type,7,:atom,[]},[],~c"definition.erl"),{:record,:r}=>{{~c"definition.erl",1},[{1,[{:x,{:type,7,:atom,[]},:erl_types.t_atom()}]}]}}
 env=Path.join(dir,id<>".env.json")
 File.write!(env,:json.encode(%{original|"type_tables"=>CoreExport.export_terms(tables|>Map.put(:scope_lookup,a)|>Map.put(:redirect_definition,b)),"exported_types"=>CoreExport.export_terms([{:scope_lookup,:rt,arity}|exports])}))
 args=case arity do 0->"";1->"integer()";2->"integer(),atom()" end
 source=Path.join(dir,id<>".erl");File.write!(source,"-module(#{id}).\n-export([f/0]).\n-spec f()->scope_lookup:rt(#{args}).\nf()->ok.\n")
 {:ok,m}=:compile.file(String.to_charlist(source),[:debug_info,{:outdir,String.to_charlist(dir)}])
 {:ok,{^m,[debug_info: {:debug_info_v1,backend,metadata},abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(String.to_charlist(Path.join(dir,id<>".beam")),[:debug_info,:abstract_code])
 {:ok,unclean}=backend.debug_info(:core_v1,m,metadata,[:no_copt,:to_core,:binary,:return_errors,:no_inline]);c=:dialyzer_clean_core.clean(unclean)
 path=Path.join(dir,id<>".core.json");File.write!(path,:json.encode(%{version: 6,profile: CoreExport.profile(),modules: [CoreExport.export_module(m,forms,c)]}))
 %{id: id,core: Path.expand(path),environment: Path.expand(env)}
end
File.write!(Path.join(dir,"requests.jsonl"),Enum.map(rows,fn r->[:json.encode(r),"\n"]end))
