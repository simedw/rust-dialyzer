Code.require_file("core_export.exs", __DIR__)

[prior, output] = System.argv()
header = prior |> File.read!() |> :erlang.binary_to_term()
plt = :dialyzer_cplt.from_file(String.to_charlist(prior))
try do
  tables = for index <- 1..5, do: {"table_#{index}", :ets.tab2list(elem(plt, index))}
  values = [{"header", header} | tables] ++ [
    {"scalars", {nil, true, :native_atom, :erlang.bsl(1, 130), -0.0, 1.25, <<0, 255>>, <<7::size(3)>>}},
    {"improper", [1, {:nested, :value} | :tail]},
    {"maps", Map.new(1..128, fn value -> {{:key, value}, [value, :ok]} end)}
  ]
  results = Enum.map(values, fn {name, value} ->
    captured = CoreExport.capture_terms(value)
    checked = CoreExport.export_terms(value)
    if captured !== checked, do: raise("checked/captured export differs: #{name}")
    %{name: name, terms: length(captured.terms), exact: true}
  end)
  report = %{checked_exports_exact: true, cases: results, prior_sha256: Base.encode16(:crypto.hash(:sha256, File.read!(prior)), case: :lower)}
  File.write!(output, :json.encode(report))
  IO.puts(:json.encode(report))
after
  :dialyzer_plt.delete(plt)
end
