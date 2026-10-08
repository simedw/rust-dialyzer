defmodule WarningCacheOracle do
  def forms(module) do
    {:ok, {^module, [abstract_code: {:raw_abstract_v1, forms}]}} = :beam_lib.chunks(:code.which(module), [:abstract_code])
    forms
  end
  def find({:match, _, {:var, _, name}, rhs}, name), do: rhs
  def find(tuple, name) when is_tuple(tuple), do: find(Tuple.to_list(tuple), name)
  def find(list, name) when is_list(list), do: Enum.find_value(list, &find(&1, name))
  def find(_, _), do: nil
  def expression(forms, function, variable) do
    form = Enum.find(forms, &match?({:function, _, ^function, _, _}, &1))
    find(form, variable) || raise("native expression absent")
  end
  def eval(expression, bindings) do
    env = Enum.reduce(bindings, :erl_eval.new_bindings(), fn {key, value}, env -> :erl_eval.add_binding(key, value, env) end)
    {:value, value, _} = :erl_eval.expr(expression, env)
    value
  end
  def warning(%{"module" => module, "warning" => token}) do
    m = String.to_atom(module)
    {:warn_return_no_exit, {~c"probe.erl", 1, {m, :f, 0}}, {:token, token}}
  end
  def token({_, _, {:token, token}}), do: token
  def warnings(value) when value in [nil, :null], do: :none
  def warnings(list), do: Enum.map(list, &warning/1)
  def stored(value) when value in [nil, :null], do: :none
  def stored(map), do: Map.new(map, fn {m, values} -> {String.to_atom(m), Enum.map(values, &warning(%{"module" => m, "warning" => &1}))} end)
  def encode(:none), do: :null
  def encode(map), do: Map.new(map, fn {m, ws} -> {Atom.to_string(m), Enum.map(ws, &token/1)} end)
  def dependencies(map), do: :dict.from_list(Enum.map(map, fn {m, ds} -> {String.to_atom(m), Enum.map(ds, &String.to_atom/1) |> Enum.sort()} end))
  def load do
    forms = forms(:dialyzer_iplt)
    copy = Enum.map(forms, fn {:attribute, a, :module, _} -> {:attribute, a, :module, :rust_warning_cache_native}; form -> form end)
    {:ok, :rust_warning_cache_native, binary} = :compile.forms(copy, [:binary, :export_all])
    {:module, :rust_warning_cache_native} = :code.load_binary(:rust_warning_cache_native, ~c"warning_cache", binary)
    {expression(forms(:dialyzer_incremental), :handle_return_and_print, :WarningsToReport), expression(forms, :to_file_custom_vsn, :CombinedModDeps)}
  end
end
{selection, dependencies} = WarningCacheOracle.load()
for line <- IO.stream(:stdio, :line) do
  r = :json.decode(line)
  collected = WarningCacheOracle.warnings(r["collected"])
  unknown = Enum.map(r["unknown"], &{String.to_atom(&1["module"]), WarningCacheOracle.warning(&1)})
  merged = :rust_warning_cache_native.merge_warnings(collected, unknown, WarningCacheOracle.stored(r["stored"]))
  selected = if merged == :none, do: :null, else: WarningCacheOracle.eval(selection, [WarningModules: Enum.map(r["modules"], &String.to_atom/1), AllWarningsMap: merged]) |> Enum.map(&WarningCacheOracle.token/1)
  deps = WarningCacheOracle.eval(dependencies, [OldModDeps: WarningCacheOracle.dependencies(r["old_dependencies"]), NewModDeps: WarningCacheOracle.dependencies(r["new_dependencies"])])
  depmap = Map.new(:dict.to_list(deps), fn {m, ds} -> {Atom.to_string(m), Enum.map(ds, &Atom.to_string/1)} end)
  IO.puts(:json.encode(%{id: r["id"], merged: WarningCacheOracle.encode(merged), selected: selected, dependencies: depmap}))
end
