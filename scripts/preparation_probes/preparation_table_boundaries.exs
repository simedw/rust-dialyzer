Code.require_file("../core_export.exs",__DIR__)
[base,core,scope,dir]=System.argv();File.mkdir_p!(dir)
original=base|>File.read!()|>:json.decode();tables=CoreExport.rebuild_terms(original["type_tables"]);exports=CoreExport.rebuild_terms(original["exported_types"])
cases=[{"nonmap_root",37},{"nonatom_key",Map.put(tables,37,%{})},{"nonmap_scalar",Map.put(tables,:badmap_import,37)},{"nonmap_tuple",Map.put(tables,:badmap_import,{:bad,:table})},{"empty_table",Map.put(tables,:badmap_import,%{})},{"reached_empty_table",Map.put(tables,:scope_lookup,%{})}]
rows=for {id,value}<-cases do
 env=Path.join(dir,id<>".env.json");File.write!(env,:json.encode(%{original|"type_tables"=>CoreExport.export_terms(value),"exported_types"=>CoreExport.export_terms(if id=="reached_empty_table",do: [{:scope_lookup,:rt,0}|exports],else: exports)}))
 %{id: id,core: Path.expand(if id=="reached_empty_table",do: scope,else: core),environment: Path.expand(env)}
end
File.write!(Path.join(dir,"requests.jsonl"),Enum.map(rows,fn r->[:json.encode(r),"\n"]end))
