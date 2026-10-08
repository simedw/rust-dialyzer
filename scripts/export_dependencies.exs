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
  def callback_lists({:plt, _info, _types, _contracts, callbacks, _exported_types}) do
    :ets.tab2list(callbacks)
    |> Enum.map(fn {module, rows} -> %{module: Atom.to_string(module), mfas: Enum.map(rows, fn {name, _} -> mfa(name) end)} end)
    |> Enum.sort_by(& &1.module)
  end
  def run(path, output) do
    plt = :dialyzer_cplt.from_file(String.to_charlist(path))
    try do
      modules = :dialyzer_plt.all_modules(plt) |> :sets.to_list() |> Enum.sort()
      contracts = :dialyzer_plt.get_all_contracts(plt)
      summaries = for module <- modules,
                      {:value, entries} = :dialyzer_plt.lookup_module(plt, module),
                      {name, result, arguments} <- entries do
        %{mfa: mfa(name), result: NativeTypes.term(result), arguments: Enum.map(arguments, &NativeTypes.term/1)}
      end
      {:ok, info} = :dialyzer.plt_info(String.to_charlist(path))
      fingerprints = for file <- Keyword.fetch!(info, :files) do
        %{path: List.to_string(file), sha256: :crypto.hash(:sha256, File.read!(file)) |> Base.encode16(case: :lower)}
      end
      artifact = %{version: 3, profile: CoreExport.profile(), source: %{exporter_sha256: :crypto.hash(:sha256, File.read!(__ENV__.file)) |> Base.encode16(case: :lower), kind: "classic_plt", path: Path.expand(path), sha256: :crypto.hash(:sha256, File.read!(path)) |> Base.encode16(case: :lower), files: fingerprints},
        modules: Enum.map(modules, &Atom.to_string/1), summaries: summaries,
        contracts: Enum.map(Enum.sort(contracts), fn {name, value} -> %{mfa: mfa(name), contract: contract(value)} end),
        callbacks: Enum.map(:dialyzer_plt.get_all_callbacks(plt) |> Enum.sort(), fn {name, value} -> %{mfa: mfa(name), contract: contract(value)} end),
        callback_lists: callback_lists(plt),
        type_tables: :dialyzer_plt.get_all_types(plt) |> CoreExport.export_terms(),
        exported_types: :dialyzer_plt.get_exported_types(plt) |> :sets.to_list() |> CoreExport.export_terms()}
      File.mkdir_p!(Path.dirname(output))
      File.write!(output, :json.encode(artifact))
      IO.puts(:json.encode(%{modules: length(modules), summaries: length(summaries), contracts: map_size(contracts), source: Path.expand(path)}))
    after
      :dialyzer_plt.delete(plt)
    end
  end
end
[path, output] = System.argv()
ExportDependencies.run(path, output)
