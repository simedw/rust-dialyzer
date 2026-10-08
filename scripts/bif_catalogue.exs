Code.require_file("native_types.exs", __DIR__)

defmodule Catalogue do
  def annotation({line, _}), do: line
  def annotation(line) when is_integer(line), do: line
  def annotation(other), do: other

  def nodes(value) when is_tuple(value), do: [value | value |> Tuple.to_list() |> Enum.flat_map(&nodes/1)]
  def nodes(value) when is_list(value), do: Enum.flat_map(value, &nodes/1)
  def nodes(_), do: []

  def mfa([{:atom, _, module}, {:atom, _, function}, {:integer, _, arity} | _]), do: {module, function, arity}
  def mfa(_), do: nil
  def key({module, function, arity}), do: "#{module}:#{function}/#{arity}"
  def key({function, arity}), do: "#{function}/#{arity}"
  def json_mfa({m, f, a}), do: %{module: Atom.to_string(m), function: Atom.to_string(f), arity: a}

  def calls(tree, imports, functions) do
    nodes(tree) |> Enum.flat_map(fn
      {:call, _, {:atom, _, f}, args} ->
        fa = {f, length(args)}
        case {f, mfa(args)} do
          {:type, target} when not is_nil(target) -> [{:transfer, target}]
          {:arg_types, target} when not is_nil(target) -> [{:domain, target}]
          _ -> cond do
            Map.has_key?(imports, fa) -> [{:external, {imports[fa], f, length(args)}}]
            Map.has_key?(functions, fa) -> [{:local, fa}]
            true -> [{:external, {:erlang, f, length(args)}}]
          end
        end
      {:call, _, {:remote, _, {:atom, _, m}, {:atom, _, f}}, args} -> [{:external, {m, f, length(args)}}]
      {:call, _, _, _} -> [{:dynamic, :unresolved}]
      _ -> []
    end) |> Enum.uniq() |> Enum.sort()
  end

  def expand(deps, graph, seen \\ MapSet.new()) do
    Enum.reduce(deps, {MapSet.new(), seen}, fn dep, {all, visited} ->
      case dep do
        {:local, fa} ->
          if MapSet.member?(visited, fa) or fa in [{:type, 3}, {:type, 4}, {:arg_types, 3}] do
            {MapSet.put(all, dep), visited}
          else
            nested = expand(Map.get(graph, fa, []), graph, MapSet.put(visited, fa))
            visited = MapSet.put(visited, fa)
            {MapSet.union(MapSet.put(all, dep), MapSet.new(nested)), visited}
          end
        {:transfer, _} ->
          if MapSet.member?(visited, dep) do
            {MapSet.put(all, dep), visited}
          else
            nested = expand(Map.get(graph, dep, []), graph, MapSet.put(visited, dep))
            {MapSet.union(MapSet.put(all, dep), MapSet.new(nested)), MapSet.put(visited, dep)}
          end
        _ -> {MapSet.put(all, dep), visited}
      end
    end) |> elem(0) |> MapSet.to_list() |> Enum.sort()
  end

  def dependency({kind, target}) when kind in [:external, :transfer, :domain], do: %{kind: Atom.to_string(kind), target: key(target)}
  def dependency({:local, fa}), do: %{kind: "helper", target: key(fa)}
  def dependency({:dynamic, _}), do: %{kind: "dynamic_call", target: "unresolved"}

  def run(source, output) do
    Code.ensure_loaded!(:erl_types)
    {:ok, forms} = :epp.parse_file(String.to_charlist(source), [], [])
    if Enum.any?(forms, &match?({:error, _}, &1)), do: raise("source preprocessing failed")
    functions = Map.new(for {:function, _, name, arity, clauses} <- forms, do: {{name, arity}, clauses})
    imports = for {:attribute, _, :import, {module, names}} <- forms, name <- names, into: %{}, do: {name, module}
    graph = Map.new(functions, fn {fa, clauses} -> {fa, calls(clauses, imports, functions)} end)
    selected = for fa <- [{:type, 4}, {:arg_types, 3}],
      {{:clause, ann, patterns, guards, body}, index} <- Enum.with_index(functions[fa]), do:
      %{function: key(fa), index: index, line: annotation(ann), target: mfa(patterns),
        patterns: patterns, guards: guards, body: body}
    fixed = Enum.filter(selected, & &1.target)
    targets = fixed |> Enum.map(& &1.target) |> Enum.uniq() |> Enum.sort()
    graph = Map.merge(graph, Map.new(targets, fn target ->
      transfer = Enum.filter(fixed, &(&1.target == target and &1.function == "type/4"))
      {{:transfer, target}, calls(Enum.map(transfer, & &1.body), imports, functions)}
    end))
    generic = Enum.reject(selected, & &1.target)
    fallback = Enum.filter(generic, fn c -> Enum.all?(Enum.take(c.patterns, 3), &match?({:var, _, _}, &1)) end)
    unresolved = generic -- fallback

    pinned = :catalogue_pinned_erl_bif_types
    renamed = Enum.map(forms, fn
      {:attribute, line, :module, _} -> {:attribute, line, :module, pinned}
      form -> form
    end)
    {:ok, ^pinned, binary, compiler_warnings} = :compile.forms(renamed, [:binary, :return_errors, :return_warnings])
    {:module, ^pinned} = :code.load_binary(pinned, String.to_charlist(source), binary)

    entries = Enum.map(targets, fn target = {m, f, a} ->
      clauses = Enum.filter(fixed, &(&1.target == target))
      domain = :erl_bif_types.arg_types(m, f, a)
      result = :erl_bif_types.type(m, f, a)
      true = domain === apply(pinned, :arg_types, [m, f, a])
      true = result === apply(pinned, :type, [m, f, a])
      known = :erl_bif_types.is_known(m, f, a)
      true = known === apply(pinned, :is_known, [m, f, a])
      transfer = Enum.filter(clauses, &(&1.function == "type/4"))
      domains = Enum.filter(clauses, &(&1.function == "arg_types/3"))
      direct = calls(Enum.map(transfer, & &1.body), imports, functions)
      deps = expand(direct, graph)
      type_functions = for {:external, {:erl_types, name, arity}} <- deps, do: key({name, arity})
      callback = Enum.any?(type_functions, &(&1 in ["t_fun_args/1", "t_fun_range/1"]))
      groups = Enum.flat_map(type_functions, fn function ->
        cond do
          String.starts_with?(function, "t_map") -> ["maps"]
          String.starts_with?(function, "t_tuple") -> ["tuples"]
          String.starts_with?(function, "t_bitstr") or String.starts_with?(function, "t_binary") -> ["bitstrings"]
          String.starts_with?(function, "t_list") or String.starts_with?(function, "t_cons") or String.starts_with?(function, "t_nonempty_list") -> ["lists"]
          String.starts_with?(function, "t_fun") -> ["functions"]
          String.starts_with?(function, "t_integer") or String.starts_with?(function, "t_number") or String.starts_with?(function, "t_from_range") or String.starts_with?(function, "number_") -> ["numbers"]
          true -> []
        end
      end) |> Enum.concat(if callback, do: ["callback_dependent"], else: []) |> Enum.uniq() |> Enum.sort()
      %{mfa: json_mfa(target), native_is_known: known, admissible_domains: NativeTypes.domains(domain),
        return_on_any: NativeTypes.type(result), specialized_return_transfer: transfer != [],
        specialized_domain_clauses: length(domains), transfer_clauses: length(transfer),
        source_clauses: Enum.map(clauses, fn c ->
          %{function: c.function, index: c.index, line: c.line, head: NativeTypes.term(c.patterns), guards: NativeTypes.term(c.guards)}
        end), direct_dependencies: Enum.map(direct, &dependency/1),
        transitive_dependencies: Enum.map(deps, &dependency/1), type_api_dependencies: Enum.sort(type_functions),
        callback_dependent: callback,
        callback_argument_positions: if(callback and domain != :unknown, do:
          (domain |> Enum.with_index(1) |> Enum.filter(fn {type, _} -> not :erl_types.t_is_any(type) and :erl_types.t_is_fun(:erl_types.t_inf(type, :erl_types.t_fun())) end) |> Enum.map(&elem(&1, 1))), else: []),
        clips_to_admissible_domains: {:local, {:strict, 5}} in deps,
        transfer_groups: groups}
    end)

    system_source = Enum.filter(fixed, &(&1.target == {:erlang, :system_info, 1} and &1.function == "type/4"))
    system_cases = system_source |> Enum.flat_map(&nodes(&1.body)) |> Enum.filter(fn
      {:case, _, {:call, _, {:atom, _, :t_atom_vals}, [{:var, _, :Type}]}, _} -> true
      _ -> false
    end)
    [system_case] = system_cases
    {:case, _, _, selector_clauses} = system_case
    selector_returns = Enum.flat_map(selector_clauses, fn {:clause, ann, patterns, guards, _body} ->
      keys = (nodes(patterns) ++ nodes(guards)) |> Enum.flat_map(fn
        {:atom, _, value} -> [value]
        _ -> []
      end) |> Enum.reject(&(&1 == :unknown)) |> Enum.uniq()
      Enum.map(keys, fn selector ->
        input = :erl_types.t_atom(selector)
        result = :erl_bif_types.type(:erlang, :system_info, 1, [input])
        true = result === apply(pinned, :type, [:erlang, :system_info, 1, [input]])
        %{selector: Atom.to_string(selector), line: annotation(ann),
          source_head: NativeTypes.term(patterns), source_guards: NativeTypes.term(guards), return: NativeTypes.type(result)}
      end)
    end) |> Enum.sort_by(& &1.selector)

    extra_modules = [:erlang, :lists, :maps, :math]
    export_probe = Enum.map(extra_modules, fn module ->
      Code.ensure_loaded!(module)
      exported = apply(module, :module_info, [:exports]) |> Enum.reject(fn {f, _} -> f == :module_info end)
      known = Enum.filter(exported, fn {f, a} -> :erl_bif_types.is_known(module, f, a) end)
      outside = Enum.reject(known, fn {f, a} -> {module, f, a} in targets end)
      if outside != [], do: raise("known module exports missing from clause catalogue #{inspect(outside)}")
      %{module: Atom.to_string(module), exported_count: length(exported), known_count: length(known),
        known_exports: Enum.map(Enum.sort(known), fn {f, a} -> key({module, f, a}) end),
        non_specialized_exports: Enum.map(Enum.sort(exported -- known), fn {f, a} -> key({module, f, a}) end)}
    end)

    # Export chunks inspect available modules without triggering on_load callbacks.
    available = Enum.map(:code.all_available(), fn {name, path, _loaded} ->
      module = List.to_atom(name)
      exports = case :beam_lib.chunks(path, [:exports]) do
        {:ok, {^module, [{:exports, exports}]}} -> exports
        _ -> []
      end
      {module, exports}
    end)
    preloaded = Enum.map(:erlang.pre_loaded(), fn module ->
      {module, apply(module, :module_info, [:exports])}
    end)
    runtime_bifs = (available ++ preloaded)
      |> Enum.flat_map(fn {module, exports} ->
        for {function, arity} <- exports,
            :erlang.is_builtin(module, function, arity), do: {module, function, arity}
      end)
      |> Enum.uniq() |> Enum.sort()
    true = {:erlang, :get_module_info, 1} in runtime_bifs
    true = {:erlang, :get_module_info, 2} in runtime_bifs

    result = %{version: 1, source: Path.expand(source), source_sha256: Base.encode16(:crypto.hash(:sha256, File.read!(source)), case: :lower),
      otp: System.otp_release(), erts: List.to_string(:erlang.system_info(:version)),
      native_module: List.to_string(:code.which(:erl_bif_types)), compiler_warnings: NativeTypes.term(compiler_warnings),
      scope: "union of exact specialized type/4 and arg_types/3 MFA heads, not module export enumeration",
      counts: %{mfas: length(entries), known: Enum.count(entries, & &1.native_is_known),
        specialized_return: Enum.count(entries, & &1.specialized_return_transfer),
        transfer_only: Enum.count(entries, &(&1.specialized_return_transfer and not &1.native_is_known)),
        callback_dependent: Enum.count(entries, & &1.callback_dependent),
        type_clauses: length(functions[{:type, 4}]), arg_types_clauses: length(functions[{:arg_types, 3}]),
        exact_clauses: length(fixed), fallback_clauses: length(fallback), unresolved_dynamic_families: length(unresolved)},
      native_validation: %{all_exact_mfas_match_pinned_source: true, checked_mfas: length(targets)},
      generic_fallbacks: Enum.map(fallback, fn c -> %{function: c.function, index: c.index, line: c.line,
        head: NativeTypes.term(c.patterns), guards: NativeTypes.term(c.guards), body: NativeTypes.term(c.body)} end),
      unresolved_dynamic_families: Enum.map(unresolved, fn c -> %{function: c.function, index: c.index, line: c.line,
        head: NativeTypes.term(c.patterns), guards: NativeTypes.term(c.guards)} end),
      runtime_builtin_inventory: %{scope: "available BEAM export chunks plus preloaded runtime exports, filtered by erlang:is_builtin/3",
        available_modules: length(available), preloaded_modules: length(preloaded),
        count: length(runtime_bifs), mfas: Enum.map(runtime_bifs, &json_mfa/1)},
      system_info_selector_returns: selector_returns,
      exported_module_crosschecks: export_probe,
      helper_dependency_graph: Map.new(graph, fn {fa, deps} -> {if(match?({:transfer, _}, fa), do: "transfer:" <> key(elem(fa, 1)), else: key(fa)), Enum.map(deps, &dependency/1)} end),
      entries: entries}
    File.mkdir_p!(Path.dirname(output))
    File.write!(output, :json.encode(result))
    File.write!(Path.join(Path.dirname(output), "source_forms.etf"), :erlang.term_to_binary(forms))
    IO.puts(:json.encode(result.counts))
  end
end

case System.argv() do
  [source, output] -> Catalogue.run(source, output)
  _ -> raise "usage: elixir scripts/generate.exs PINNED_ERL_BIF_TYPES_SOURCE OUTPUT_JSON"
end
