# Compare complete records and encoded bytes from two exporters.
# elixir verify_core_export.exs REPO BEAM_LIST --baseline EXPORTER [--candidate EXPORTER] [--limit N]
{opts, [repo, beam_list], []} =
  OptionParser.parse(System.argv(), strict: [limit: :integer, baseline: :string, candidate: :string])
original = Path.join(repo, "scripts/core_export.exs")
Code.require_file(original)
baseline_path = opts[:baseline] || raise "--baseline exporter required"
candidate_path = opts[:candidate] || original
baseline = File.read!(baseline_path)
  |> String.replace("defmodule CoreExport do", "defmodule BaselineCoreExport do", global: false)
Code.compile_string(baseline, original)
Code.compiler_options(ignore_module_conflict: true)
Code.compile_string(File.read!(candidate_path), original)

defmodule CoreExportVerification do
  @options [:no_copt, :to_core, :binary, :return_errors, :no_inline,
            :strict_record_tests, :dialyzer, :no_spawn_compiler_process]

  def check(path) do
    bytes = File.read!(path)
    {:ok, {name, [debug_info: {:debug_info_v1, backend, metadata}, abstract_code: {:raw_abstract_v1, forms}]}} =
      :beam_lib.chunks(bytes, [:debug_info, :abstract_code])
    {:ok, unclean} = backend.debug_info(:core_v1, name, metadata, @options)
    core = :dialyzer_clean_core.clean(unclean)
    baseline = BaselineCoreExport.build_module(name, forms, core, :etf)
    candidate = CoreExport.build_module(name, forms, core, :etf)
    true = baseline === candidate
    baseline_bytes = :erlang.term_to_binary(baseline)
    candidate_bytes = :erlang.term_to_binary(candidate)
    true = baseline_bytes === candidate_bytes
    true = BaselineCoreExport.build_module(name, forms, core, :json) ===
      CoreExport.build_module(name, forms, core, :json)
    %{module: Atom.to_string(name), etf_record_exact: true, etf_bytes_exact: true,
      json_record_exact: true, etf_bytes: byte_size(candidate_bytes)}
  end
end

true = BaselineCoreExport.profile() === CoreExport.profile()
beams = beam_list |> File.read!() |> String.split("\n", trim: true)
limit = opts[:limit] || length(beams)
beams |> Enum.take(limit) |> Enum.each(fn beam ->
  IO.puts(:json.encode(CoreExportVerification.check(beam)))
end)
