# Measures how much of a PLT can influence a project's warnings.
#
# Usage: elixir cone.exs <plt> <project_ebin> [out.json [plt.beams cone.beams]]
#
# Dialyzer refines types per module, so the unit of reachability is the module:
# start from the modules the project calls, add every module those call, repeat.
# Edges come from the PLT's own mod_deps (callee => callers), inverted, and from
# xref's static remote calls for the project modules.

[plt_path, ebin | rest] = System.argv()

{:file_plt, _vsn, md5s, _info, _contracts, _callbacks, _types, _exported, mod_deps, _impl} =
  plt_path |> File.read!() |> :erlang.binary_to_term()

callees =
  mod_deps
  |> :dict.to_list()
  |> Enum.reduce(%{}, fn {callee, callers}, acc ->
    Enum.reduce(callers, acc, fn caller, acc ->
      Map.update(acc, caller, MapSet.new([callee]), &MapSet.put(&1, callee))
    end)
  end)

plt_modules =
  for {file, _md5} <- md5s, into: MapSet.new(), do: file |> Path.basename(".beam") |> String.to_atom()

{:ok, _} = :xref.start(:cone, xref_mode: :modules)
:ok = :xref.set_default(:cone, warnings: false, verbose: false)
{:ok, project_modules} = :xref.add_directory(:cone, String.to_charlist(ebin))
{:ok, project_calls} = :xref.q(:cone, ~c"ME")
:xref.stop(:cone)

project = MapSet.new(project_modules)

seeds =
  for {from, to} <- project_calls, from in project, to not in project, into: MapSet.new(), do: to

reach = fn reach, frontier, seen ->
  case MapSet.difference(frontier, seen) |> MapSet.to_list() do
    [] ->
      seen

    new ->
      seen = Enum.into(new, seen)
      next = new |> Enum.flat_map(&Map.get(callees, &1, [])) |> MapSet.new()
      reach.(reach, next, seen)
  end
end

cone =
  reach.(reach, MapSet.intersection(seeds, plt_modules), MapSet.new())
  |> MapSet.intersection(plt_modules)

:code.add_pathsa(Path.wildcard(Path.join(ebin, "../../*/ebin")) |> Enum.map(&String.to_charlist/1))

module_app =
  for dir <- :code.get_path(),
      app_file <- Path.wildcard(Path.join(to_string(dir), "*.app")),
      {:ok, [{:application, app, props}]} = :file.consult(app_file),
      m <- Keyword.get(props, :modules, []),
      into: %{},
      do: {m, app}

app_of = &Map.get(module_app, &1, :unknown)

by_app = fn mods ->
  mods |> Enum.group_by(app_of) |> Map.new(fn {a, ms} -> {a, length(ms)} end)
end

result = %{
  plt_modules: MapSet.size(plt_modules),
  project_modules: MapSet.size(project),
  direct_plt_callees: MapSet.size(MapSet.intersection(seeds, plt_modules)),
  callees_outside_plt: MapSet.size(MapSet.difference(seeds, plt_modules)),
  cone_modules: MapSet.size(cone),
  plt_by_app: by_app.(plt_modules),
  cone_by_app: by_app.(cone)
}

IO.puts(
  "PLT modules: #{result.plt_modules}, project modules: #{result.project_modules}, " <>
    "direct callees: #{result.direct_plt_callees}, cone: #{result.cone_modules} " <>
    "(#{Float.round(100 * result.cone_modules / result.plt_modules, 1)}%)"
)

write_beams = fn path, keep ->
  lines = for {file, _} <- md5s, keep.(file |> Path.basename(".beam") |> String.to_atom()), do: [file, ?\n]
  File.write!(path, lines)
end

case rest do
  [out | beams] ->
    File.write!(out, :json.encode(result))

    with [all, cone_out] <- beams do
      write_beams.(all, fn _ -> true end)
      write_beams.(cone_out, &(&1 in cone))
    end

  [] ->
    :ok
end
