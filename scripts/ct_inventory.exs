[root, output] = System.argv()

defmodule CTInventory do
  def expand(items, groups) do
    Enum.flat_map(items, fn
      {:group, name} ->
        {^name, _, children} = Enum.find(groups, fn {group, _, _} -> group == name end)
        expand(children, groups)

      name when is_atom(name) ->
        [Atom.to_string(name)]

      other ->
        raise "unsupported CT registry entry #{inspect(other)}"
    end)
  end
end

suites =
  Path.wildcard(Path.join(root, "lib/dialyzer/test/*_SUITE.erl"))
  |> Enum.map(fn source ->
    compiled =
      :compile.file(String.to_charlist(source), [:binary, :return_errors, :return_warnings])

    {module, binary} =
      case compiled do
        {:ok, module, binary} -> {module, binary}
        {:ok, module, binary, _} -> {module, binary}
        error -> raise "cannot inventory #{source}: #{inspect(error)}"
      end

    {:module, ^module} = :code.load_binary(module, String.to_charlist(source), binary)
    groups = if function_exported?(module, :groups, 0), do: module.groups(), else: []

    %{
      suite: Atom.to_string(module),
      file: source,
      cases: CTInventory.expand(module.all(), groups)
    }
  end)

File.write!(output, :json.encode(suites))

