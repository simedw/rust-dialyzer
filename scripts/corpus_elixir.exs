Code.require_file("beam_export.exs", __DIR__)

defmodule CorpusElixir do
  @base_modules [
    :elixir,
    :elixir_env,
    :elixir_erl_pass,
    :maps,
    :sets,
    ArgumentError,
    Atom,
    Code,
    Enum,
    Exception,
    ExUnit.AssertionError,
    ExUnit.Assertions,
    IO,
    Kernel,
    Kernel.Utils,
    List,
    Macro,
    Macro.Env,
    MapSet,
    Module,
    Protocol,
    String,
    String.Chars,
    Task,
    Task.Supervisor
  ]

  defp json_value(nil), do: :null

  defp json_value(value) when is_map(value),
    do: Map.new(value, fn {key, item} -> {key, json_value(item)} end)

  defp json_value(value) when is_list(value), do: Enum.map(value, &json_value/1)
  defp json_value(value), do: value

  def run(elixir_root, dialyxir_root, manifest_path, output_dir) do
    manifest = manifest_path |> File.read!() |> :json.decode()
    cases = if is_list(manifest), do: manifest, else: Map.fetch!(manifest, "cases")
    cases = Enum.filter(cases, &(&1["kind"] in ["elixir", "dialyxir"]))
    output_dir = Path.expand(output_dir)
    File.mkdir_p!(output_dir)
    Code.compiler_options(debug_info: true, infer_signatures: [:elixir])

    sources = %{
      "elixir" =>
        Path.wildcard(Path.join(elixir_root, "lib/elixir/test/elixir/fixtures/dialyzer/*.ex")),
      "dialyxir" => Path.wildcard(Path.join(dialyxir_root, "test/examples/*.ex"))
    }

    compiled =
      for kind <- Enum.uniq(Enum.map(cases, & &1["kind"])), into: %{} do
        dir = Path.join(output_dir, "_#{kind}/compiled")

        result =
          protect(fn ->
            File.rm_rf!(dir)
            File.mkdir_p!(dir)
            if sources[kind] == [], do: raise("no #{kind} sources found")

            case Kernel.ParallelCompiler.compile_to_path(sources[kind], dir,
                   return_diagnostics: true
                 ) do
              {:ok, _, diagnostics} ->
                %{dir: dir, diagnostics: inspect(diagnostics, limit: :infinity)}

              other ->
                raise "compile failed: #{inspect(other, limit: :infinity)}"
            end
          end)

        {kind, result}
      end

    plt = protect(fn -> base_plt(output_dir) end)

    for testcase <- cases do
      case_dir = Path.join(output_dir, safe_id(testcase["id"]))
      File.mkdir_p!(case_dir)
      beams_dir = Path.join(case_dir, "beams")
      program = Path.join(case_dir, "program.json")

      base = %{
        id: testcase["id"],
        compile_status: "error",
        beams_dir: beams_dir,
        program_json: program,
        warnings: [],
        warning_options: testcase["warning_options"] || [],
        oracle_source: "OTP :dialyzer.run on compiled upstream Elixir BEAM files"
      }

      record =
        case {compiled[testcase["kind"]], plt} do
          {{:error, error}, _} ->
            Map.merge(base, %{oracle_status: "compile_error", error: error})

          {{:ok, compilation}, {:error, error}} ->
            Map.merge(base, %{
              compile_status: "ok",
              oracle_status: "oracle_error",
              error: error,
              compile_diagnostics: compilation.diagnostics
            })

          {{:ok, compilation}, {:ok, cache}} ->
            case protect(fn -> analyze(testcase, compilation, cache, beams_dir, program) end) do
              {:ok, result} ->
                Map.merge(base, result)

              {:error, error} ->
                Map.merge(base, %{
                  compile_status: "ok",
                  oracle_status: "oracle_error",
                  error: error
                })
            end
        end

      File.write!(Path.join(case_dir, "oracle.json"), :json.encode(json_value(record)))
      IO.puts("#{testcase["id"]}: #{record.oracle_status}, #{length(record.warnings)} warnings")
    end
  end

  defp analyze(testcase, compilation, cache, beams_dir, program) do
    File.rm_rf!(beams_dir)
    File.mkdir_p!(beams_dir)

    files =
      if testcase["kind"] == "dialyxir" do
        Path.wildcard(Path.join(compilation.dir, "*.beam"))
      else
        Enum.map(testcase["modules"], &Path.join(compilation.dir, "#{&1}.beam"))
      end

    if files == [], do: raise("case has no BEAM files")
    Enum.each(files, &File.cp!(&1, Path.join(beams_dir, Path.basename(&1))))
    Export.run(beams_dir, program)

    {analysis_us, warnings} =
      :timer.tc(fn ->
        :dialyzer.run(
          analysis_type: :succ_typings,
          check_plt: false,
          files_rec: [String.to_charlist(beams_dir)],
          plts: [String.to_charlist(cache.path)],
          warnings: Enum.map(testcase["warning_options"] || [], &String.to_atom/1)
        )
      end)

    modules = Enum.map(files, &read_module/1)
    normalized = Enum.map(warnings, &normalize(&1, modules))

    status =
      cond do
        testcase["kind"] == "dialyxir" -> "live_reference"
        testcase["expected"] == "clean" and warnings == [] -> "golden_match"
        true -> "golden_diff"
      end

    %{
      compile_status: "ok",
      compile_diagnostics: compilation.diagnostics,
      oracle_status: status,
      warnings: normalized,
      analysis_us: analysis_us,
      plt: cache.path,
      plt_status: cache.status,
      oracle_source:
        if(testcase["kind"] == "elixir",
          do: "Elixir Kernel.DialyzerTest upstream zero-warning assertion",
          else:
            "Live OTP :dialyzer.run; Dialyxir examples have no upstream semantic golden assertions"
        )
    }
  end

  defp base_plt(output_dir) do
    files =
      Enum.map(@base_modules, fn module ->
        path = :code.which(module)
        if not is_list(path), do: raise("missing base module #{module}")
        path
      end)

    erts_files = Path.wildcard(Path.join(List.to_string(:code.lib_dir(:erts)), "ebin/*.beam"))
    parts = Enum.map(files ++ Enum.map(erts_files, &String.to_charlist/1), &File.read!/1)

    key =
      :crypto.hash(
        :sha256,
        :erlang.term_to_binary({System.version(), System.otp_release(), parts})
      )
      |> Base.encode16(case: :lower)

    path = Path.join(output_dir, "_elixir/base-#{key}.plt")
    File.mkdir_p!(Path.dirname(path))
    valid = File.exists?(path) and match?({:ok, _}, :dialyzer.plt_info(String.to_charlist(path)))

    status =
      if valid do
        "reused"
      else
        :dialyzer.run(
          analysis_type: :plt_build,
          output_plt: String.to_charlist(path),
          apps: [:erts],
          files: files
        )

        "built"
      end

    %{path: path, status: status}
  end

  defp read_module(path) do
    {:ok, {name, [abstract_code: {:raw_abstract_v1, forms}]}} =
      :beam_lib.chunks(String.to_charlist(path), [:abstract_code])

    files = for {:attribute, _, :file, {file, _}} <- forms, do: List.to_string(file)

    functions =
      for {:function, annotation, function, arity, _} <- forms,
          function not in [:__info__, :module_info],
          do: {line(annotation), function, arity}

    %{name: name, files: files, functions: Enum.sort(functions)}
  end

  defp normalize({tag, {file, annotation}, {category, details}} = warning, modules) do
    file = if is_list(file), do: List.to_string(file), else: to_string(file)
    at = line(annotation)
    candidates = Enum.filter(modules, &(file in &1.files))

    explicit =
      case {category, details} do
        {:invalid_contract, [module, function, arity | _]} -> {module, function, arity}
        {:no_return, [_, function, arity]} -> {nil, function, arity}
        _ -> nil
      end

    module =
      case explicit do
        {name, _, _} when not is_nil(name) ->
          Enum.find(modules, &(&1.name == name))

        {_, function, arity} ->
          Enum.find(candidates, fn module ->
            Enum.any?(module.functions, fn {_, name, n} -> name == function and n == arity end)
          end)

        _ ->
          Enum.max_by(
            candidates,
            fn module ->
              module.functions
              |> Enum.filter(fn {start, _, _} -> start <= at end)
              |> Enum.map(&elem(&1, 0))
              |> Enum.max(fn -> -1 end)
            end,
            fn -> nil end
          )
      end

    symbol =
      cond do
        explicit != nil ->
          {elem(explicit, 1), elem(explicit, 2)}

        module != nil ->
          case Enum.filter(module.functions, fn {start, _, _} -> start <= at end)
               |> List.last() do
            {_, name, arity} -> {name, arity}
            _ -> {nil, nil}
          end

        true ->
          {nil, nil}
      end

    message =
      case protect(fn -> warning |> :dialyzer.format_warning() |> IO.chardata_to_string() end) do
        {:ok, text} -> text
        {:error, error} -> "Warning formatting failed: #{error}"
      end

    %{
      module: if(module, do: Atom.to_string(module.name)),
      function: if(elem(symbol, 0), do: Atom.to_string(elem(symbol, 0))),
      arity: elem(symbol, 1),
      line: at,
      file: file,
      category: Atom.to_string(category),
      tag: Atom.to_string(tag),
      message: message,
      raw_term: inspect(warning, limit: :infinity, printable_limit: :infinity)
    }
  end

  defp line({at, _}) when is_integer(at), do: at
  defp line(at) when is_integer(at), do: at
  defp line(annotation), do: :erl_anno.line(annotation)
  defp safe_id(id), do: String.replace(id, ~r/[^a-zA-Z0-9_-]/, "__")

  defp protect(fun) do
    try do
      {:ok, fun.()}
    rescue
      error -> {:error, Exception.format(:error, error, __STACKTRACE__)}
    catch
      kind, error -> {:error, Exception.format(kind, error, __STACKTRACE__)}
    end
  end
end

case System.argv() do
  [elixir_root, dialyxir_root, manifest, output_dir] ->
    CorpusElixir.run(elixir_root, dialyxir_root, manifest, output_dir)

  _ ->
    raise "usage: elixir scripts/corpus_elixir.exs ELIXIR_ROOT DIALYXIR_ROOT MANIFEST OUTPUT_DIR"
end

