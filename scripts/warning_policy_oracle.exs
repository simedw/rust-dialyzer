Code.require_file("core_export.exs", __DIR__)
for path <- IO.stream(:stdio, :line) do
  input = path |> String.trim() |> File.read!() |> :json.decode()
  result = try do
    modules = Enum.map(input["modules"], fn mod ->
      {_, core} = CoreExport.rebuild_module(mod)
      name = :cerl.module_name(core) |> :cerl.concrete()
      props = :dialyzer_utils.get_fun_meta_info(name, core, :dialyzer_options.build([]) |> elem(13))
      case props do
        {:error, message} -> throw({:invalid, message})
        props ->
          module_tags = Keyword.fetch!(props, name)
          functions = for {{m, f, a}, rules} <- props do
            suppressed = Keyword.get(rules, :nowarn_function) == :func
            tags = if suppressed, do: module_tags, else: Enum.reduce(rules, module_tags, fn {tag, provenance}, acc ->
              case provenance do
                :mod -> List.delete(acc, tag)
                :func -> Enum.sort(Enum.uniq([tag | acc]))
              end
            end)
            %{function: %{module: Atom.to_string(m), name: Atom.to_string(f), arity: a}, profile: %{tags: Enum.map(tags, &Atom.to_string/1)}, suppressed: suppressed}
          end
          %{module: Atom.to_string(name), profile: %{tags: Enum.map(module_tags, &Atom.to_string/1)}, functions: functions}
      end
    end)
    %{Ok: %{modules: modules}}
  catch
    kind, reason -> %{Err: %{kind: kind, reason: inspect(reason)}}
  end
  IO.puts(:json.encode(result))
end
