[root]=System.argv()
true=Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(:code.which(:cerl_trees)))))=="F9C9D8C2DBCD7C2F170CA67124072A047D15B0DA5A9C4F81C270D111BFE6E727"
true=Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(:code.which(:cerl)))))=="0BE66918002098F37A5DDC7DE7AE2DF27A239E0889DA44F8EF08AF0C69B5073E"
Code.require_file(Path.join(root,"scripts/core_export.exs"))
for line<-IO.stream(:stdio,:line) do
 r=:json.decode(line);core=r["core_json"]|>File.read!()|>:json.decode()
 {captures,next}=Enum.map_reduce(core["modules"],Map.get(r,"initial",0),fn module,next->
  {_,tree}=CoreExport.rebuild_module(module)
  {labeled,next}=:cerl_trees.label(tree,next)
  {CoreExport.export_terms([labeled]),next}
 end)
 File.write!(r["native_json"],:json.encode(%{captures: captures,next_label: next}))
 IO.puts(:json.encode(%{id: r["id"],modules: length(captures),next_label: next}))
end
