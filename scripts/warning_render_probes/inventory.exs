Code.require_file("../core_export.exs",__DIR__)
root=hd(System.argv())
manifest=:json.decode(File.read!(Path.join(root,".cache/engine/manifest.json")))
{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer),[:abstract_code])
{:function,_,:message_to_string,3,clauses}=Enum.find(forms,&match?({:function,_,:message_to_string,3,_},&1))
messages=Enum.map(clauses,fn {:clause,a,[pattern,_,_],guards,body}->%{line: :erl_anno.line(a),pattern: CoreExport.export_terms(pattern),guards: CoreExport.export_terms(guards),body: CoreExport.export_terms(body)}end)
File.write!(".cache/warning_render/source-messages.json",:json.encode(%{version: 1,beam_sha256: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(:code.which(:dialyzer)))),case: :lower),clauses: messages}))
selected=[:message_to_string,:format_invalid_contract_details,:call_or_apply_to_string,:form_positions,:form_expected_without_opaque,:form_expected,:form_position_string,:ordinal,:con,:a,:c,:rec_type,:ps]
functions=Enum.filter(forms,fn f->match?({:function,_,_,_,_},f) and elem(f,2) in selected end)
File.write!(".cache/warning_render/source-functions.json",:json.encode(CoreExport.export_terms(functions)))

rows=Enum.flat_map(manifest,fn entry->data=:json.decode(File.read!(entry["native_json"]));warnings=CoreExport.rebuild_terms(data["warnings"]);Enum.with_index(warnings)|>Enum.map(fn {warning,index}->%{id: entry["id"]<>"/"<>Integer.to_string(index),warning: warning}end)end)
counts=Enum.frequencies_by(rows,fn %{warning: {_,_,{kind,_}}}->Atom.to_string(kind)end)
IO.puts(:json.encode(%{cases: length(manifest),warnings: length(rows),categories: counts,source_clauses: length(clauses)}))
File.write!(".cache/warning_render/render.inventory.json",:json.encode(%{cases: length(manifest),warnings: length(rows),categories: counts,source_clauses: length(clauses)}))
outputs=for row<-rows,indent<-[false,true],filename<-[:basename,:fullpath],location<-[:line,:column] do
 opts=[indent_opt: indent,filename_opt: filename,error_location: location]
 text=:dialyzer.format_warning(row.warning,opts)|>List.to_string()
 %{id: row.id,options: %{indent: indent,filename: Atom.to_string(filename),location: Atom.to_string(location)},warning: CoreExport.export_terms(row.warning),expected: text}
end
File.write!(".cache/warning_render/render.native.jsonl",Enum.map_join(outputs,"\n",&:json.encode/1)<>"\n")
