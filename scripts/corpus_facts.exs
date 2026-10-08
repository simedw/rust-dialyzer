Code.require_file("native_types.exs", __DIR__)
Code.require_file("core_export.exs", __DIR__)

defmodule CorpusFacts do
  def sha(path), do: :crypto.hash(:sha256, File.read!(path)) |> Base.encode16(case: :lower)
  def options(%{"kind" => "otp", "suite_dir" => directory}) do
    {:ok, terms} = :file.consult(String.to_charlist(Path.join(directory, "dialyzer_options")))
    terms = case terms do [list] when is_list(list) -> list; other -> other end
    Keyword.get(terms, :dialyzer_options, []) |> Keyword.drop([:include_dirs, :defines, :pa, :files, :files_rec])
  end
  def options(case_info), do: [warnings: Enum.map(case_info["warning_options"] || [], &String.to_atom/1)]

  def analyze(c, output, key) do
    files = Path.wildcard(Path.join(c["beams_dir"], "*.beam")) |> Enum.map(&String.to_charlist/1)
    opts = options(c)
    plt_file = Path.join(output, "target.plt")
    filtered_file = Path.join(output, "filtered-base.plt")
    targets = MapSet.new(Enum.map(files, fn file ->
      {:ok, {module, _}} = :beam_lib.chunks(file, [:exports])
      Atom.to_string(module)
    end))
    {:ok, base_info} = :dialyzer.plt_info(String.to_charlist(c["base_plt"]))
    collisions = Keyword.fetch!(base_info, :files) |> Enum.filter(fn file ->
      MapSet.member?(targets, file |> List.to_string() |> Path.basename(".beam"))
    end)
    base = if collisions == [] do
      c["base_plt"]
    else
      :dialyzer.run(analysis_type: :plt_remove, files: collisions, init_plt: String.to_charlist(c["base_plt"]), output_plt: String.to_charlist(filtered_file), check_plt: false, get_warnings: false)
      filtered_file
    end
    try do
      :dialyzer.run([analysis_type: :plt_add, files: files, init_plt: String.to_charlist(base), output_plt: String.to_charlist(plt_file), check_plt: false, get_warnings: false] ++ opts)
      plt = :dialyzer_cplt.from_file(String.to_charlist(plt_file))
      signatures = try do
        for file <- files,
            {:ok, {module, _}} = :beam_lib.chunks(file, [:exports]),
            {:value, summaries} = :dialyzer_plt.lookup_module(plt, module),
            {{^module, function, arity}, result, arguments} <- summaries do
          %{module: Atom.to_string(module), name: Atom.to_string(function), arity: arity,
            result: NativeTypes.type(result), arguments: Enum.map(arguments, &NativeTypes.type/1)}
        end
      after
        :dialyzer_plt.delete(plt)
      end
      warnings = :dialyzer.run([files: files, init_plt: String.to_charlist(c["base_plt"]), check_plt: false] ++ opts)
      %{version: 1, id: c["id"], key: key, status: "complete", profile: CoreExport.profile(),
        options: CoreExport.export_terms(opts), signatures: signatures,
        warnings: CoreExport.export_terms(warnings), messages: Enum.map(warnings, fn w -> :dialyzer.format_warning(w, opts) |> List.to_string() end)}
    after
      File.rm(plt_file)
      File.rm(filtered_file)
    end
  end

  def run(manifest, destination) do
    {:ok, supervisor} = Task.Supervisor.start_link()
    cases = manifest |> File.read!() |> :json.decode()
    for c <- cases do
      output = Path.join(destination, c["slug"])
      File.mkdir_p!(output)
      path = Path.join(output, "native.json")
      dependencies = [c["base_plt"], c["core_json"], __ENV__.file, Path.join(__DIR__, "native_types.exs"), Path.join(__DIR__, "core_export.exs")] ++ Path.wildcard(Path.join(c["beams_dir"], "*.beam"))
      key = :crypto.hash(:sha256, :erlang.term_to_binary({Enum.map(dependencies, &{&1, sha(&1)}), options(c), CoreExport.profile()})) |> Base.encode16(case: :lower)
      cached = if File.exists?(path), do: path |> File.read!() |> :json.decode(), else: %{}
      if cached["key"] == key and cached["status"] == "complete" do
        IO.puts("#{c["id"]}: native facts reused")
      else
        task = Task.Supervisor.async_nolink(supervisor, fn -> analyze(c, output, key) end)
        result = case Task.yield(task, 60_000) || Task.shutdown(task, :brutal_kill) do
          {:ok, value} -> value
          {:exit, reason} -> %{id: c["id"], key: key, status: "error", reason: inspect(reason, limit: :infinity)}
          nil -> %{id: c["id"], key: key, status: "timeout"}
        end
        File.write!(path, :json.encode(result))
        IO.puts("#{c["id"]}: native facts #{result.status}")
      end
    end
  end
end
[manifest, output] = System.argv()
CorpusFacts.run(manifest, output)
