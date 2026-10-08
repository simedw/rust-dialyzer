[baseline, candidate, prior, output] = System.argv()
Code.compile_string(baseline |> File.read!() |> String.replace("defmodule ExportDependencies do", "defmodule BaselineExportDependencies do"), baseline)
Code.compile_string(candidate |> File.read!() |> String.replace("defmodule ExportDependencies do", "defmodule CandidateExportDependencies do"), candidate)
:logger.set_primary_config(:level, :none)
plt = :dialyzer_cplt.from_file(String.to_charlist(prior))
try do
  bytes = File.read!(prior)
  header = :erlang.binary_to_term(bytes)
  reports = for compact <- [false, true] do
    old = BaselineExportDependencies.build(plt, prior, compact)
    standalone = CandidateExportDependencies.build(plt, prior, compact)
    shared = CandidateExportDependencies.build(plt, prior, compact, {bytes, header})
    normalize = fn artifact -> update_in(artifact.source, &Map.delete(&1, :exporter_sha256)) end
    if normalize.(old) !== normalize.(standalone) or normalize.(old) !== normalize.(shared),
      do: raise("complete dependency artifacts differ")
    %{compact: compact, modules: length(old.modules), exact: true}
  end
  File.write!(output, :json.encode(%{cases: reports, all_dependency_fields_exact: true,
    exclusion: "exporter_sha256 identifies deliberately different source files"}))
after
  :dialyzer_plt.delete(plt)
end
