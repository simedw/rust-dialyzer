Code.require_file("core_export.exs",__DIR__)
for line<-IO.stream(:stdio,:line) do
 row=:json.decode(line); value=CoreExport.rebuild_terms(row["raw"])
 IO.puts(:json.encode(Map.put(row,"ok",:erl_types.t_is_opaque(value,String.to_atom(row["module"])))))
end
