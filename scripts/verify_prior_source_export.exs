[baseline, candidate, prior, output] = System.argv()
Code.compile_string(baseline |> File.read!() |> String.replace("defmodule ExportDependencies do", "defmodule BaselineDependencies do"), baseline)
Code.require_file(candidate)
:logger.set_primary_config(:level, :none)
plt = :dialyzer_cplt.from_file(String.to_charlist(prior))
try do
  {baseline_us, baseline} = :timer.tc(fn -> BaselineDependencies.build(plt, prior, true) end)
  {candidate_us, candidate} = :timer.tc(fn -> ExportDependencies.build(plt, prior, true) end)
  strip = fn artifact -> Map.update!(artifact, :source, &Map.delete(&1, :exporter_sha256)) end
  if strip.(baseline) !== strip.(candidate), do: raise("complete dependency export differs")
  {:ok, native} = :dialyzer.plt_info(String.to_charlist(prior))
  header = prior |> File.read!() |> :erlang.binary_to_term()
  from_header = for {file, _digest} <- elem(header, 2), do: file
  if from_header !== Keyword.fetch!(native, :files), do: raise("Native source file order differs")
  report = %{complete_export_exact: true, native_source_files_exact: true,
    source_files: length(from_header), baseline_seconds: baseline_us / 1_000_000,
    candidate_seconds: candidate_us / 1_000_000,
    excluded_field: "source.exporter_sha256 identifies the intentionally changed helper"}
  File.write!(output, :json.encode(report))
  IO.puts(:json.encode(report))
after
  :dialyzer_plt.delete(plt)
end
