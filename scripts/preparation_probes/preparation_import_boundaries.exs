Code.require_file("../core_export.exs",__DIR__)
[base,dir]=System.argv();File.mkdir_p!(dir)
original=base|>File.read!()|>:json.decode()
tables=CoreExport.rebuild_terms(original["type_tables"]);exports=CoreExport.rebuild_terms(original["exported_types"])
field={:a,{:type,1,:integer,[]},:erl_types.t_integer()}
body={:type,1,:record,[{:atom,1,:r}]}
variants=[{1,[field]},{2,[field,{:b,{:type,1,:atom,[]},:erl_types.t_atom()}]}]
cases=[{"unused_multi",false,variants,body},{"used_multi",true,variants,body},{"used_single",true,Enum.take(variants,1),body},{"unused_bad_body",false,Enum.take(variants,1),37},{"used_bad_body",true,Enum.take(variants,1),37},{"unused_bad_cached",false,[{1,[{:a,elem(field,1),37}]}],body},{"used_bad_cached",true,[{1,[{:a,elem(field,1),37}]}],body}]
rows=for {id,used,variants,body}<-cases do
 table=%{{:record,:r}=>{{~c"prep_boundary_lib.erl",1},variants},{:type,:rt,0}=>{{:prep_boundary_lib,{~c"prep_boundary_lib.erl",2},body,[]},:any}}
 env=Path.join(dir,id<>".env.json")
 File.write!(env,:json.encode(%{original|"type_tables"=>CoreExport.export_terms(Map.put(tables,:prep_boundary_lib,table)),"exported_types"=>CoreExport.export_terms([{:prep_boundary_lib,:rt,0}|exports])}))
 source=Path.join(dir,id<>".erl")
 File.write!(source,"-module(#{id}).\n-export([f/0]).\n-spec f()->#{if used,do: "prep_boundary_lib:rt()",else: "atom()"}.\nf()->ok.\n")
 {:ok,m}=:compile.file(String.to_charlist(source),[:debug_info,{:outdir,String.to_charlist(dir)}])
 {:ok,{^m,[debug_info: {:debug_info_v1,backend,metadata},abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(String.to_charlist(Path.join(dir,id<>".beam")),[:debug_info,:abstract_code])
 {:ok,unclean}=backend.debug_info(:core_v1,m,metadata,[:no_copt,:to_core,:binary,:return_errors,:no_inline])
 c=:dialyzer_clean_core.clean(unclean)
 path=Path.join(dir,id<>".core.json");File.write!(path,:json.encode(%{version: 6,profile: CoreExport.profile(),modules: [CoreExport.export_module(m,forms,c)]}))
 %{id: id,core: Path.expand(path),environment: Path.expand(env)}
end
File.write!(Path.join(dir,"requests.jsonl"),Enum.map(rows,fn r->[:json.encode(r),"\n"]end))
