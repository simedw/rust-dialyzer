{:ok, {:dialyzer_incremental, [{:abstract_code, {:raw_abstract_v1, forms}}]}} =
  :beam_lib.chunks(:code.which(:dialyzer_incremental), [:abstract_code])
{:ok, :dialyzer_incremental, binary} = :compile.forms(forms, [:binary, :export_all])
{:module, :dialyzer_incremental} = :code.load_binary(:dialyzer_incremental, ~c"invalidation_probe", binary)
for line <- IO.stream(:stdio, :line) do
  request = :json.decode(line)
  changed = Enum.map(request["changed"], &String.to_atom/1)
  removed = Enum.map(request["removed"], &String.to_atom/1)
  diff = Enum.map(changed, &{:differ, &1}) ++ Enum.map(removed, &{:removed, &1})
  dependencies = :dict.from_list(Enum.map(request["dependencies"], fn {module, deps} ->
    {String.to_atom(module), Enum.map(deps, &String.to_atom/1) |> Enum.sort()}
  end))
  paths = Map.new(request["current"], fn module ->
    {String.to_atom(module), String.to_charlist("/probe/" <> module <> ".beam")}
  end)
  {files, discard, remaining} = :dialyzer_incremental.expand_dependent_modules([], diff, dependencies, paths)
  output = %{analyze: Enum.map(files, fn file -> file |> List.to_string() |> Path.basename(".beam") end) |> Enum.sort(),
    discard: :sets.to_list(discard) |> Enum.map(&Atom.to_string/1) |> Enum.sort(),
    remaining_dependencies: Map.new(:dict.to_list(remaining), fn {module, deps} ->
      {Atom.to_string(module), Enum.map(deps, &Atom.to_string/1) |> Enum.sort()}
    end)}
  IO.puts(:json.encode(output))
end
