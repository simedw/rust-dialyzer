Code.require_file("../core_export.exs",__DIR__)
for {module,alias_name} <- [{:dialyzer_cl,:warning_cl},{:dialyzer_incremental,:warning_incremental}] do
 {:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(module),[:abstract_code])
 forms=Enum.map(forms,fn {:attribute,a,:module,_}->{:attribute,a,:module,alias_name};f->f end)
 {:ok,^alias_name,bin,_}=:compile.forms(forms,[:binary,:export_all,:return_errors,:return_warnings])
 {:module,^alias_name}=:code.load_binary(alias_name,~c"ordering",bin)
end
profile=Map.new([:dialyzer_cl,:dialyzer_incremental],fn m->{m,Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(:code.which(m)))),case: :lower)}end)
root=hd(System.argv());manifest=:json.decode(File.read!(Path.join(root,".cache/engine/manifest.json")))
all=Enum.flat_map(manifest,fn entry->data=:json.decode(File.read!(entry["native_json"]));CoreExport.rebuild_terms(data["warnings"])end)
base=[{:tag_a,{~c"f.erl",1,{:m,:f,0}},{:message,[~c"x"]}},{:tag_b,{~c"f.erl",1,{:m,:f,0}},{:message,[~c"x"]}},{:tag_a,{~c"f.erl",1,{:m,:f,0}},{:message,[~c"y"]}},{:tag_a,{~c"f.erl",{1,2},{:m,:f,0}},{:message,[~c"x"]}}]
sequences=for a<-base,b<-base,c<-base,d<-base,do: [a,b,c,d]
values=[1,1.0,-0.0,0.0,1234567890123456789012345678901234567890,-1.5,:a,:b,{}, {1,1.0},[],[1],[1|:tail],<<>>,<<1>>,<<1::size(3)>>,<<1::size(9)>>,%{1=>:integer,1.0=>:float},%{0.0=>:positive,-0.0=>:negative}]
raw=for value<-values,do: {:tag,{~c"raw.erl",{1,1},{:m,:f,0}},value}
raw_sequences=for a<-raw,b<-raw,do: [a,b,a,b,b,a]
rows=[all,Enum.reverse(all),all++all]++sequences++[raw,Enum.reverse(raw),raw++raw]++raw_sequences
for {warnings,index}<-Enum.with_index(rows) do
 classic=:warning_cl.process_warnings(warnings);incremental=:warning_incremental.process_warnings(warnings)
 true=classic==incremental
 IO.puts(:json.encode(%{id: index,profile: profile,input: CoreExport.export_terms(warnings),expected: CoreExport.export_terms(classic)}))
end
