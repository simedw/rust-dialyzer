# Small exact-byte checks for memo safety. No analyzer or cache files.
# elixir verify_core_value_memo.exs REPO --baseline EXPORTER [--candidate EXPORTER]
{opts, [repo], []} = OptionParser.parse(System.argv(), strict: [baseline: :string, candidate: :string])
original = Path.join(repo, "scripts/core_export.exs")
Code.require_file(original)
baseline = File.read!(opts[:baseline] || raise("--baseline exporter required"))
  |> String.replace("defmodule CoreExport do", "defmodule BaselineCoreExport do", global: false)
Code.compile_string(baseline, original)
Code.compiler_options(ignore_module_conflict: true)
Code.compile_string(File.read!(opts[:candidate] || original), original)

<<negative_zero::float-64>> = <<0x8000000000000000::64>>
safe = {:safe, {:nested, [1, 2 | :tail], <<5::3>>, <<0, 1, 2>>}}
pairs = for key <- 1..48, do: {key, {key, key + 1}}
map_a = Map.new(pairs)
map_b = Map.new(Enum.reverse(pairs))
cases = [
  {"safe_repeat", List.duplicate(safe, 16)},
  {"float_bits", [safe, {:float, 0.0}, {:float, negative_zero}, {:float, 0.0}, {:float, negative_zero}]},
  {"map_provenance", [safe, {:map, map_a}, {:map, map_b}, {:map, map_a}]},
  {"unsafe_ancestors", [{{:deep, [{:float, negative_zero}]}, map_a},
    {{:deep, [{:float, 0.0}]}, map_b}, {{:deep, [{:float, negative_zero}]}, map_a}]}
]
Enum.each(cases, fn {label, annotations} ->
  core = {:c_module, annotations, {:c_literal, [], :memo_fixture}, [], [], []}
  forms = annotations
  baseline = BaselineCoreExport.build_module(:memo_fixture, forms, core, :etf)
  candidate = CoreExport.build_module(:memo_fixture, forms, core, :etf)
  true = baseline === candidate
  baseline_bytes = :erlang.term_to_binary(baseline)
  candidate_bytes = :erlang.term_to_binary(candidate)
  true = baseline_bytes === candidate_bytes
  true = BaselineCoreExport.build_module(:memo_fixture, forms, core, :json) ===
    CoreExport.build_module(:memo_fixture, forms, core, :json)
  IO.puts(:json.encode(%{fixture: label, etf_record_exact: true, etf_bytes_exact: true,
    json_record_exact: true, etf_bytes: byte_size(candidate_bytes)}))
end)
