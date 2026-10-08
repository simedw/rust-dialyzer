Code.require_file("core_export.exs", __DIR__)

defmodule CoreCorpus do
  def run(report_path, output_path, core_filename \\ "core.json") do
    report = report_path |> File.read!() |> :json.decode()
    results = Enum.map(report["cases"], fn entry ->
      try do
        oracle = entry["oracle_file"] |> File.read!() |> :json.decode()
        directory = Path.dirname(oracle["program_json"])
        output = Path.join(directory, core_filename)
        result = CoreExport.run(Path.join(directory, "beams"), output)
        %{id: entry["id"], status: "roundtrip", modules: result.modules, core_json: output}
      rescue
        error -> %{id: entry["id"], status: "failure", reason: Exception.message(error)}
      catch
        kind, reason -> %{id: entry["id"], status: "failure", reason: inspect({kind, reason})}
      end
    end)
    counts = Enum.frequencies_by(results, & &1.status)
    summary = %{version: 1, profile: CoreExport.profile(), cases: results, counts: counts,
                complete: Enum.all?(results, &(&1.status == "roundtrip")),
                checks: ["all abstract forms retained", "Erlang term exact JSON roundtrip",
                         "Core node and annotation exact JSON roundtrip", "OTP compiler profile equality"],
                analyzer_parity: false}
    File.mkdir_p!(Path.dirname(output_path))
    File.write!(output_path, :json.encode(summary))
    IO.inspect(counts)
    if !summary.complete, do: System.halt(1)
  end
end

case System.argv() do
  [report, output, filename] -> CoreCorpus.run(report, output, filename)
  [report, output] -> CoreCorpus.run(report, output)
  _ -> raise("usage: elixir scripts/core_corpus.exs CORPUS_REPORT.json OUTPUT.json")
end
