Code.require_file("native_types.exs", __DIR__)
Code.require_file("core_export.exs", __DIR__)

defmodule ExportDependencies do
  def mfa({module, function, arity}), do: %{module: Atom.to_string(module), function: Atom.to_string(function), arity: arity}
  def contract({location, value, flags}), do: Map.merge(contract(value), %{location: NativeTypes.term(location), flags: NativeTypes.term(flags)})
  def contract({:contract, alternatives, arguments, forms}) do
    %{alternatives: Enum.map(alternatives, fn {signature, constraints} ->
        %{signature: NativeTypes.term(signature), constraints: Enum.map(constraints, fn {:subtype, left, right} -> %{left: NativeTypes.term(left), right: NativeTypes.term(right)} end)}
      end), arguments: Enum.map(arguments, &NativeTypes.term/1), forms: CoreExport.export_terms(forms)}
  end
  def build(plt, path), do: build(plt, path, false)
  def build(plt, path, compact) do
      capture = if compact, do: &CoreExport.capture_terms/1, else: &CoreExport.export_terms/1
      modules = :dialyzer_plt.all_modules(plt) |> :sets.to_list() |> Enum.sort()
      contracts = :dialyzer_plt.get_all_contracts(plt)
      summaries = for module <- modules,
                      {:value, entries} = :dialyzer_plt.lookup_module(plt, module),
                      {name, result, arguments} <- entries do
        if compact, do: {name, result, arguments}, else: %{mfa: mfa(name), result: NativeTypes.term(result), arguments: Enum.map(arguments, &NativeTypes.term/1)}
      end
      prior_bytes = File.read!(path)
      header = :erlang.binary_to_term(prior_bytes)
      {source_kind, files} = case elem(header, 0) do
        :file_plt ->
          {"classic_plt", for({file, _digest} <- elem(header, 2), do: file)}
        :ifile_plt -> {"incremental_plt", []}
      end
      {fingerprints, unavailable} = Enum.reduce(files, {[], []}, fn file, {available, missing} ->
        path = IO.chardata_to_string(file)
        case File.read(file) do
          {:ok, bytes} -> {[ %{path: path, sha256: :crypto.hash(:sha256, bytes) |> Base.encode16(case: :lower)} | available], missing}
          {:error, reason} -> {available, [%{path: path, reason: Atom.to_string(reason)} | missing]}
        end
      end)
      fingerprints = Enum.reverse(fingerprints)
      unavailable = Enum.reverse(unavailable)
      artifact = %{version: if(compact, do: 5, else: 4), profile: CoreExport.profile(), source: %{exporter_sha256: :crypto.hash(:sha256, File.read!(__ENV__.file)) |> Base.encode16(case: :lower), kind: source_kind, path: Path.expand(path), sha256: :crypto.hash(:sha256, prior_bytes) |> Base.encode16(case: :lower), files: fingerprints, unavailable_files: unavailable},
        modules: Enum.map(modules, &Atom.to_string/1), summaries: if(compact, do: [], else: summaries),
        contracts: if(compact, do: [], else: Enum.map(Enum.sort(contracts), fn {name, value} -> %{mfa: mfa(name), contract: contract(value)} end)),
        raw_callback_groups: capture.(:ets.tab2list(elem(plt, 4))),
        type_tables: :dialyzer_plt.get_all_types(plt) |> capture.(),
        exported_types: :dialyzer_plt.get_exported_types(plt) |> :sets.to_list() |> capture.()}
      if compact, do: Map.merge(artifact, %{raw_summaries: capture.(summaries), raw_contracts: capture.(Enum.sort(contracts))}), else: artifact
  end

  def raw_tables(plt), do: for(i <- 1..5, do: :ets.tab2list(elem(plt, i)))

  def run(path, output) do
    plt = :dialyzer_cplt.from_file(String.to_charlist(path))
    try do
      artifact = build(plt, path)
      File.mkdir_p!(Path.dirname(output))
      File.write!(output, :json.encode(artifact))
      IO.puts(:json.encode(%{modules: length(artifact.modules), summaries: length(artifact.summaries), contracts: length(artifact.contracts), source: Path.expand(path)}))
    after
      :dialyzer_plt.delete(plt)
    end
  end
end
