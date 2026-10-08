defmodule Oracle do
  def line({line, _column}), do: line
  def line(line) when is_integer(line), do: line

  def run(beam_dir, output, cache_dir) do
    File.mkdir_p!(cache_dir)
    files = Path.wildcard(Path.join(beam_dir, "*.beam")) |> Enum.sort()
    if files == [], do: raise("no BEAM files found in #{beam_dir}")
    beams = Enum.map(files, &String.to_charlist/1)
    erlang_beam = Path.join(List.to_string(:code.lib_dir(:erts)), "ebin/erlang.beam")

    runtime_key =
      hash([System.otp_release(), :erlang.system_info(:version), File.read!(erlang_beam)])

    base = Path.join(cache_dir, "base-#{runtime_key}.plt")

    {base_us, base_status} =
      :timer.tc(fn ->
        ensure_plt(base, fn ->
          :dialyzer.run(
            analysis_type: :plt_build,
            files: [String.to_charlist(erlang_beam)],
            output_plt: String.to_charlist(base),
            get_warnings: false,
            warnings: [:no_unknown]
          )
        end)
      end)

    app_key =
      hash([
        runtime_key | Enum.flat_map(files, fn path -> [Path.expand(path), File.read!(path)] end)
      ])

    app = Path.join(cache_dir, "app-#{app_key}.plt")

    {app_us, app_status} =
      :timer.tc(fn ->
        ensure_plt(app, fn ->
          :dialyzer.run(
            analysis_type: :plt_add,
            files: beams,
            init_plt: String.to_charlist(base),
            output_plt: String.to_charlist(app),
            check_plt: false,
            get_warnings: false,
            warnings: [:no_unknown]
          )
        end)
      end)

    {analysis_us, warnings} =
      :timer.tc(fn ->
        :dialyzer.run(
          analysis_type: :succ_typings,
          files: beams,
          init_plt: String.to_charlist(app),
          check_plt: false,
          warnings: [:no_unknown]
        )
      end)

    modules = Enum.map(files, &read_module/1)

    records =
      Enum.map(warnings, &normalize(&1, modules))
      |> Enum.sort_by(&{&1.module, &1.function, &1.arity, &1.line, &1.category})

    result = %{
      version: 1,
      warnings: records,
      raw_warnings:
        Enum.map(warnings, fn warning ->
          %{
            term: inspect(warning, limit: :infinity, printable_limit: :infinity),
            message: warning |> :dialyzer.format_warning() |> List.to_string()
          }
        end),
      cache: %{base_plt: base, app_plt: app, base_status: base_status, app_status: app_status},
      timings_us: %{base_plt: base_us, app_plt: app_us, analysis: analysis_us}
    }

    File.mkdir_p!(Path.dirname(output))
    File.write!(output, :json.encode(result))
  end

  defp hash(parts),
    do: :crypto.hash(:sha256, :erlang.term_to_binary(parts)) |> Base.encode16(case: :lower)

  defp ensure_plt(path, build) do
    if File.exists?(path) do
      {:ok, _} = :dialyzer.plt_info(String.to_charlist(path))
      "reused"
    else
      build.()
      {:ok, _} = :dialyzer.plt_info(String.to_charlist(path))
      "built"
    end
  end

  defp read_module(path) do
    {:ok, {name, [abstract_code: {:raw_abstract_v1, forms}]}} =
      :beam_lib.chunks(String.to_charlist(path), [:abstract_code])

    {:attribute, _, :file, {file, _}} = Enum.find(forms, &match?({:attribute, _, :file, _}, &1))

    functions =
      for {:function, annotation, name, arity, _} <- forms,
          name != :__info__,
          do: {line(annotation), name, arity}

    %{name: name, file: file, functions: Enum.sort(functions)}
  end

  defp normalize({_tag, {file, annotation}, {category, details}}, modules) do
    at = line(annotation)

    module =
      Enum.find(modules, &(&1.file == file)) || raise("unknown warning file #{inspect(file)}")

    {name, arity} =
      case {category, details} do
        {:invalid_contract, [_module, name, arity | _]} ->
          {name, arity}

        {:no_return, [_reason, name, arity]} ->
          {name, arity}

        {:call, _} ->
          {_line, name, arity} =
            module.functions |> Enum.filter(fn {start, _, _} -> start <= at end) |> List.last()

          {name, arity}

        other ->
          raise "unsupported warning #{inspect(other)}"
      end

    %{
      module: Atom.to_string(module.name),
      function: Atom.to_string(name),
      arity: arity,
      line: at,
      category: Atom.to_string(category)
    }
  end
end

case System.argv() do
  [beam_dir, output, cache_dir] -> Oracle.run(beam_dir, output, cache_dir)
  _ -> raise "usage: elixir scripts/oracle.exs BEAM_DIRECTORY OUTPUT_JSON CACHE_DIRECTORY"
end

