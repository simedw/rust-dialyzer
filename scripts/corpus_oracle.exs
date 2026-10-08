Code.require_file("beam_export.exs", __DIR__)

defmodule CorpusOracle do
  defp term(value),
    do: inspect(value, limit: :infinity, printable_limit: :infinity, charlists: :as_lists)

  defp chars(value), do: String.to_charlist(value)
  defp text(value) when is_list(value), do: List.to_string(value)
  defp text(value), do: to_string(value)
  defp line({n, _}) when is_integer(n), do: n
  defp line(n) when is_integer(n), do: n
  defp line(annotation), do: :erl_anno.line(annotation)
  defp json_value(nil), do: :null

  defp json_value(value) when is_map(value),
    do: Map.new(value, fn {key, item} -> {key, json_value(item)} end)

  defp json_value(value) when is_list(value), do: Enum.map(value, &json_value/1)
  defp json_value(value), do: value
  defp write(path, result), do: File.write!(path, :json.encode(json_value(result)))

  defp hash(value),
    do: :crypto.hash(:sha256, :erlang.term_to_binary(value)) |> Base.encode16(case: :lower)

  def run(otp, manifest_path, output, seconds) do
    File.mkdir_p!(output)
    helpers = Path.join(output, "helpers")
    File.mkdir_p!(helpers)
    test_dir = Path.join(otp, "lib/dialyzer/test")

    for name <- ["file_utils", "dialyzer_common"] do
      source = chars(Path.join(test_dir, name <> ".erl"))

      case :compile.file(source, [:return_errors, :return_warnings, {:outdir, chars(helpers)}]) do
        {:ok, _, _} -> :ok
        {:ok, _} -> :ok
        error -> raise("cannot compile upstream helper: #{term(error)}")
      end
    end

    true = :code.add_patha(chars(helpers))
    base = ensure_plt(output)
    {:ok, supervisor} = Task.Supervisor.start_link()
    manifest = manifest_path |> File.read!() |> :json.decode()

    for record <- manifest["cases"] do
      id = record["id"]
      safe = String.replace(id, ~r/[^a-zA-Z0-9_.-]/, "_") <> "-" <> String.slice(hash(id), 0, 10)
      case_out = Path.join(output, safe)
      File.rm_rf!(case_out)
      File.mkdir_p!(case_out)
      File.ln_s!(base, Path.join(case_out, "dialyzer_plt"))
      task = Task.Supervisor.async_nolink(supervisor, fn -> analyze(record, case_out, base) end)

      result =
        case Task.yield(task, seconds * 1000) || Task.shutdown(task, :brutal_kill) do
          {:ok, value} -> value
          {:exit, reason} -> failure(record, case_out, "oracle_error", term(reason))
          nil -> failure(record, case_out, "timeout", "case exceeded #{seconds} seconds")
        end

      write(Path.join(case_out, "oracle.json"), result)
      IO.puts("#{id}: #{result.oracle_status}; compile=#{result.compile_status}")
    end
  end

  defp failure(record, out, status, detail) do
    %{
      id: record["id"],
      oracle_status: status,
      golden_detail: detail,
      compile_status: "error",
      compile_detail: "Compilation did not complete before the oracle failed or timed out",
      beams_dir: Path.join(out, "beams"),
      options_term: nil,
      warnings: [],
      source_warnings: [],
      source_beam_equal: nil,
      program_json: nil
    }
  end

  defp ensure_plt(output) do
    :application.load(:dialyzer)
    apps = [:erts, :kernel, :stdlib]

    beam_files =
      Enum.flat_map(apps, fn app ->
        :code.lib_dir(app) |> text() |> Path.join("ebin/*.beam") |> Path.wildcard()
      end)
      |> Enum.sort()

    {:ok, version} = :application.get_key(:dialyzer, :vsn)

    metadata = %{
      otp: System.otp_release(),
      erts: text(:erlang.system_info(:version)),
      dialyzer: text(version),
      apps: Enum.map(apps, &Atom.to_string/1),
      fingerprint: hash(Enum.map(beam_files, &{&1, hash(File.read!(&1))})),
      beam_count: length(beam_files)
    }

    base = Path.join(output, "base.plt")
    metadata_path = base <> ".json"

    cached =
      if File.exists?(metadata_path), do: File.read!(metadata_path) |> :json.decode(), else: nil

    matching = cached == :json.encode(metadata) |> IO.iodata_to_binary() |> :json.decode()
    reuse = matching and File.exists?(base) and match?({:ok, _}, :dialyzer.plt_info(chars(base)))

    if reuse do
      IO.puts("full OTP PLT reused: #{base}")
    else
      IO.puts("building full erts/kernel/stdlib PLT: #{base}")
      temporary = base <> ".building"
      File.rm(temporary)

      :dialyzer.run(
        analysis_type: :plt_build,
        apps: apps,
        output_plt: chars(temporary),
        warnings: [:no_unknown],
        get_warnings: false
      )

      {:ok, _} = :dialyzer.plt_info(chars(temporary))
      File.rename!(temporary, base)
      write(metadata_path, metadata)
      IO.puts("full OTP PLT built")
    end

    base
  end

  defp options(suite_dir) do
    {:ok, terms} = :file.consult(chars(Path.join(suite_dir, "dialyzer_options")))

    terms =
      case terms do
        [list] when is_list(list) -> list
        other -> other
      end

    Keyword.get(terms, :dialyzer_options, [])
  end

  defp resolved_options(opts, suite_dir) do
    Enum.reduce(opts, [], fn
      {:pa, path}, acc ->
        true = :code.add_patha(chars(Path.join(suite_dir, text(path))))
        acc

      {key, paths}, acc when key in [:include_dirs, :files_rec, :files] ->
        [{:include_dirs, Enum.map(paths, &chars(Path.join(suite_dir, text(&1))))} | acc]

      option, acc ->
        [option | acc]
    end)
  end

  defp analyze(record, out, base) do
    try do
      suite_dir = record["suite_dir"]
      opts = options(suite_dir)
      resolved = resolved_options(opts, suite_dir)
      files = Enum.map(record["files"], &chars/1)

      golden =
        :dialyzer_common.check(String.to_atom(record["case"]), opts, chars(suite_dir), chars(out))

      {status, detail} =
        case golden do
          :same -> {"golden_match", nil}
          {:differ, _, _} -> {"golden_diff", term(golden)}
          other -> {"oracle_error", term(other)}
        end

      beam_dir = Path.join(out, "beams")
      File.mkdir_p!(beam_dir)

      compile_opts =
        [:binary, :debug_info, :return_errors, :return_warnings] ++
          Enum.map(Keyword.get(resolved, :include_dirs, []), &{:i, &1}) ++
          Enum.map(Keyword.get(resolved, :defines, []), fn
            {name, value} -> {:d, name, value}
            name -> {:d, name}
          end)

      compiled =
        Enum.map(files, fn file ->
          result =
            case :compile.file(file, compile_opts) do
              {:ok, module, binary} when is_binary(binary) ->
                File.write!(Path.join(beam_dir, "#{module}.beam"), binary)
                {:ok, module}

              {:ok, module, binary, warnings} when is_binary(binary) ->
                File.write!(Path.join(beam_dir, "#{module}.beam"), binary)
                {:ok, module, warnings}

              other ->
                other
            end

          {text(file), result}
        end)

      compile_ok =
        Enum.all?(compiled, fn
          {_, {:ok, _}} -> true
          {_, {:ok, _, _}} -> true
          _ -> false
        end)

      modules = read_modules(beam_dir)

      source_raw =
        :dialyzer.run(
          [files: files, from: :src_code, init_plt: chars(base), check_plt: false] ++ resolved
        )

      source_warnings = Enum.map(source_raw, &normalize(&1, modules, resolved))

      {beam_status, beam_raw, beam_error} =
        if compile_ok do
          try do
            raw =
              :dialyzer.run(
                [
                  files: Enum.map(Path.wildcard(Path.join(beam_dir, "*.beam")), &chars/1),
                  from: :byte_code,
                  init_plt: chars(base),
                  check_plt: false
                ] ++ resolved
              )

            {"ok", raw, nil}
          catch
            kind, reason -> {"error", [], term({kind, reason})}
          end
        else
          {"not_run", [], nil}
        end

      warnings = Enum.map(beam_raw, &normalize(&1, modules, resolved))

      program =
        if compile_ok do
          path = Path.join(out, "program.json")
          Export.run(beam_dir, path)
          path
        end

      %{
        id: record["id"],
        oracle_status: status,
        golden_detail: detail,
        compile_status: if(compile_ok, do: "ok", else: "error"),
        compile_detail: term(compiled),
        beams_dir: beam_dir,
        options_term: term(opts),
        warnings: warnings,
        source_warnings: source_warnings,
        beam_oracle_status: beam_status,
        beam_oracle_error: beam_error,
        source_beam_equal:
          if(beam_status == "ok", do: equivalent(source_warnings, warnings), else: nil),
        program_json: program
      }
    catch
      kind, reason -> failure(record, out, "oracle_error", term({kind, reason, __STACKTRACE__}))
    end
  end

  defp read_modules(dir) do
    for path <- Path.wildcard(Path.join(dir, "*.beam")) do
      {:ok, {name, [abstract_code: {:raw_abstract_v1, forms}]}} =
        :beam_lib.chunks(chars(path), [:abstract_code])

      files = for {:attribute, _, :file, {file, _}} <- forms, do: text(file)

      functions =
        for {:function, annotation, function, arity, _} <- forms,
            do: {line(annotation), function, arity}

      %{name: name, files: files, functions: Enum.sort(functions)}
    end
  end

  defp normalize({tag, {file, annotation}, {category, details}} = raw, modules, opts) do
    at = line(annotation)
    file = text(file)

    module =
      Enum.find(modules, fn m ->
        Enum.any?(m.files, &(Path.expand(&1) == Path.expand(file)))
      end)

    explicit =
      case {category, details} do
        {:invalid_contract, [m, name, arity | _]}
        when is_atom(m) and is_atom(name) and is_integer(arity) ->
          {m, name, arity}

        {:no_return, [_, name, arity]} when is_atom(name) and is_integer(arity) ->
          {module && module.name, name, arity}

        _ ->
          nil
      end

    inferred =
      if module do
        case module.functions
             |> Enum.filter(fn {start, _, _} -> start <= at end)
             |> List.last() do
          {_, name, arity} -> {module.name, name, arity}
          nil -> {module.name, nil, nil}
        end
      else
        {nil, nil, nil}
      end

    {name, function, arity} = explicit || inferred

    %{
      module: name && Atom.to_string(name),
      function: function && Atom.to_string(function),
      arity: arity,
      line: at,
      file: file,
      category: Atom.to_string(category),
      tag: Atom.to_string(tag),
      identity_status: if(function, do: "resolved", else: "unresolved"),
      message: :dialyzer.format_warning(raw, opts) |> IO.chardata_to_string(),
      raw_term: term(raw)
    }
  end

  defp equivalent(source, beam) do
    key = fn warning ->
      {warning.module, warning.function, warning.arity, warning.line, warning.category,
       warning.tag, warning.raw_term}
    end

    Enum.sort(Enum.map(source, key)) == Enum.sort(Enum.map(beam, key))
  end
end

case System.argv() do
  [otp, manifest, output] ->
    CorpusOracle.run(Path.expand(otp), manifest, Path.expand(output), 60)

  [otp, manifest, output, seconds] ->
    CorpusOracle.run(Path.expand(otp), manifest, Path.expand(output), String.to_integer(seconds))

  _ ->
    raise(
      "usage: elixir scripts/corpus_oracle.exs OTP_ROOT MANIFEST_JSON OUTPUT_DIR [TIMEOUT_SECONDS]"
    )
end

