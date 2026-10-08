Code.require_file("core_export.exs", __DIR__)
Code.require_file("type_wire.exs", __DIR__)
for line <- IO.stream(:stdio, :line) do
  row = :json.decode(line)
  type = row["arena"] |> CoreExport.rebuild_terms() |> :erl_types.t_from_term()
  IO.puts(:json.encode(%{id: row["id"], type: NativeTypeWire.encode(type)}))
end
