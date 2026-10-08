Code.require_file("../core_export.exs",__DIR__)
[base,dir]=System.argv();File.mkdir_p!(dir)
original=base|>File.read!()|>:json.decode();tables=CoreExport.rebuild_terms(original["type_tables"]);exports=CoreExport.rebuild_terms(original["exported_types"])
body={:type,7,:record,[{:atom,7,:r}]};header={:malformed_lookup,{~c"imported.erl",1},body,[]}
field={:x,{:type,7,:integer,[]},:erl_types.t_integer()}
values=[{"scalar",37},{"bad_header",{37,37}},{"empty",{{~c"imported.erl",1},[]}},{"multi",{{~c"imported.erl",1},[{1,[field]},{2,[field,field]}]}},{"scalar_fields",{{~c"imported.erl",1},[{1,37}]}},{"improper_fields",{{~c"imported.erl",1},[{1,[field|37]}]}},{"bad_entry",{{~c"imported.erl",1},[{1,[37]}]}},{"bad_form",{{~c"imported.erl",1},[{1,[{:x,37,:any}]}]}},{"bad_cache",{{~c"imported.erl",1},[{1,[{:x,elem(field,1),37}]}]}},{"integer_name",{{~c"imported.erl",1},[{1,[{77,elem(field,1),:any}]}]}},{"tuple_name",{{~c"imported.erl",1},[{1,[{{:foreign,:name},elem(field,1),:any}]}]}},{"wrong_arity",{{~c"imported.erl",1},[{77,[field]}]}},{"foreign_arity",{{~c"imported.erl",1},[{:anything,[field]}]}},{"valid",{{~c"imported.erl",1},[{1,[field]}]}}]
rows=for {label,value}<-values,used<-[false,true] do
 id="#{if used,do: "used",else: "unused"}_#{label}"
 env=Path.join(dir,id<>".env.json");table=%{{:type,:rt,0}=>{header,:any},{:record,:r}=>value,37=>{:unrelated,:garbage},{:garbage,37,[]}=><<0,255>>}
 File.write!(env,:json.encode(%{original|"type_tables"=>CoreExport.export_terms(Map.put(tables,:malformed_lookup,table)),"exported_types"=>CoreExport.export_terms([{:malformed_lookup,:rt,0}|exports])}))
 source=Path.join(dir,id<>".erl");File.write!(source,"-module(#{id}).\n-export([f/0]).\n-spec f()->#{if used,do: "malformed_lookup:rt()",else: "integer()"}.\nf()->1.\n")
 {:ok,m}=:compile.file(String.to_charlist(source),[:debug_info,{:outdir,String.to_charlist(dir)}])
 {:ok,{^m,[debug_info: {:debug_info_v1,backend,metadata},abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(String.to_charlist(Path.join(dir,id<>".beam")),[:debug_info,:abstract_code])
 {:ok,unclean}=backend.debug_info(:core_v1,m,metadata,[:no_copt,:to_core,:binary,:return_errors,:no_inline]);c=:dialyzer_clean_core.clean(unclean)
 path=Path.join(dir,id<>".core.json");File.write!(path,:json.encode(%{version: 6,profile: CoreExport.profile(),modules: [CoreExport.export_module(m,forms,c)]}))
 %{id: id,core: Path.expand(path),environment: Path.expand(env)}
end
File.write!(Path.join(dir,"requests.jsonl"),Enum.map(rows,fn r->[:json.encode(r),"\n"]end))
