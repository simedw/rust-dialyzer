Code.require_file("compiler_atom_primitives.exs", __DIR__)

defmodule CoreExport do
  @options [:no_copt, :to_core, :binary, :return_errors, :no_inline,
            :strict_record_tests, :dialyzer, :no_spawn_compiler_process]

  def term(value) when is_atom(value), do: %{kind: "atom", value: Atom.to_string(value)}
  def term(value) when is_integer(value), do: %{kind: "integer", value: Integer.to_string(value)}
  def term(value) when is_float(value), do: %{kind: "float", value: Base.encode16(<<value::float-64>>)}
  def term(value) when is_binary(value), do: %{kind: "binary", value: Base.encode64(value)}
  def term(value) when is_bitstring(value) do
    bits = bit_size(value)
    padding = rem(8 - rem(bits, 8), 8)
    %{kind: "bitstring", bytes: Base.encode64(<<value::bitstring, 0::size(padding)>>), bits: bits}
  end
  def term([]), do: %{kind: "nil"}
  def term([_ | _] = value) do
    {items, tail} = list_parts(value, [])
    %{kind: "list", items: Enum.map(items, &term/1), tail: term(tail)}
  end
  def term(value) when is_tuple(value), do: %{kind: "tuple", items: value |> Tuple.to_list() |> Enum.map(&term/1)}
  def term(value) when is_map(value), do: %{kind: "map", entries: value |> :maps.to_list() |> Enum.sort() |> Enum.map(fn {k, v} -> [term(k), term(v)] end)}
  def term(value), do: raise("unsupported Erlang term: #{inspect(value)}")

  defp list_parts([head | tail], acc), do: list_parts(tail, [head | acc])
  defp list_parts(tail, acc), do: {Enum.reverse(acc), tail}

  def decode(%{"kind" => "atom", "value" => v}), do: String.to_atom(v)
  def decode(%{"kind" => "integer", "value" => v}), do: String.to_integer(v)
  def decode(%{"kind" => "float", "value" => v}) do
    <<float::float-64>> = Base.decode16!(v)
    float
  end
  def decode(%{"kind" => "binary", "value" => v}), do: Base.decode64!(v)
  def decode(%{"kind" => "bitstring", "bytes" => v, "bits" => bits}) do
    <<value::bitstring-size(bits), _::bitstring>> = Base.decode64!(v)
    value
  end
  def decode(%{"kind" => "nil"}), do: []
  def decode(%{"kind" => "list", "items" => items, "tail" => tail}), do: Enum.reduce(Enum.reverse(items), decode(tail), fn item, acc -> [decode(item) | acc] end)
  def decode(%{"kind" => "tuple", "items" => items}), do: items |> Enum.map(&decode/1) |> List.to_tuple()
  def decode(%{"kind" => "map", "entries" => entries}), do: Map.new(entries, fn [k, v] -> {decode(k), decode(v)} end)

  def profile do
    Application.load(:dialyzer)
    %{otp: List.to_string(:erlang.system_info(:otp_release)),
      erts: List.to_string(:erlang.system_info(:version)),
      elixir: System.version(), dialyzer: List.to_string(Application.spec(:dialyzer, :vsn)),
      term_map_order: "native_to_list_with_iterator", core_options: Enum.map(@options, &Atom.to_string/1)}
  end

  defp iterator_keys(iterator, acc) do
    case :maps.next(iterator) do
      :none -> Enum.reverse(acc)
      {key, _value, next} -> iterator_keys(next, [key | acc])
    end
  end

  defp intern(value) do
    key = cond do
      is_tuple(value) -> {:tuple, value |> Tuple.to_list() |> Enum.map(&intern/1)}
      match?([_ | _], value) ->
        {items, tail} = list_parts(value, [])
        {:list, Enum.map(items, &intern/1), intern(tail)}
      is_map(value) ->
        pairs = :maps.to_list(value)
        indices = pairs |> Enum.with_index() |> Map.new(fn {{key, _}, index} -> {key, index} end)
        order = iterator_keys(:maps.iterator(value), []) |> Enum.map(&Map.fetch!(indices, &1))
        {:map, Enum.map(pairs, fn {k, v} -> [intern(k), intern(v)] end), order}
      is_atom(value) -> {:atom, value}
      is_integer(value) -> {:integer, value}
      is_float(value) -> {:float, <<value::float-64>>}
      is_binary(value) -> {:binary, value}
      is_bitstring(value) -> {:bitstring, value}
      value == [] -> :nil
      true -> term(value)
    end
    case Map.fetch(Process.get(:core_terms_cache), key) do
      {:ok, id} -> id
      :error ->
        id = Process.get(:core_terms_count)
        Process.put(:core_terms_count, id + 1)
        entry = if Process.get(:core_export_format) == :etf, do: etf_term(key), else: arena_node(key)
        Process.put(:core_terms, [entry | Process.get(:core_terms)])
        Process.put(:core_terms_cache, Map.put(Process.get(:core_terms_cache), key, id))
        id
    end
  end

  # Mirror intern's atom visit order without retaining the source AST.
  defp intern_source_atoms(value) when is_atom(value) do
    intern(value)
    :ok
  end
  defp intern_source_atoms(value) when is_tuple(value) do
    value |> Tuple.to_list() |> Enum.each(&intern_source_atoms/1)
  end
  defp intern_source_atoms([_ | _] = value) do
    {items, tail} = list_parts(value, [])
    Enum.each(items, &intern_source_atoms/1)
    intern_source_atoms(tail)
  end
  defp intern_source_atoms(value) when is_map(value) do
    Enum.each(:maps.to_list(value), fn {key, item} ->
      intern_source_atoms(key)
      intern_source_atoms(item)
    end)
  end
  defp intern_source_atoms(_value), do: :ok

  defp arena_node({:tuple, items}), do: %{kind: "tuple", items: items}
  defp arena_node({:list, items, tail}), do: %{kind: "list", items: items, tail: tail}
  defp arena_node({:map, entries, order}), do: %{kind: "map", entries: entries, iterator_order: order}
  defp arena_node({:float, bytes}), do: %{kind: "float", value: Base.encode16(bytes)}
  defp arena_node({:atom, value}), do: term(value)
  defp arena_node({:integer, value}), do: term(value)
  defp arena_node({:binary, value}), do: term(value)
  defp arena_node({:bitstring, value}), do: term(value)
  defp arena_node(:nil), do: term([])

  # Compact cache encoding (see src/core/etf.rs): index lists become packed
  # big-endian u32 binaries, so the reader never builds per-index terms.
  defp ids(list), do: for(i <- list, into: <<>>, do: <<i::32>>)
  defp etf_term({:tuple, items}), do: {:tuple, ids(items)}
  defp etf_term({:list, items, tail}), do: {:list, ids(items), tail}
  defp etf_term({:map, entries, order}), do: {:map, ids(List.flatten(entries)), ids(order)}
  defp etf_term(:nil), do: :nil
  defp etf_term({kind, value}) when kind in [:float, :atom, :integer, :binary, :bitstring], do: {kind, value}

  defp flat_node(core) do
    if Process.get(:core_export_format) == :etf,
      do: flat_node_etf(core), else: flat_node_json(core)
  end

  defp flat_node_etf(core) do
    kind = :cerl.type(core)
    annotations = intern(:cerl.get_ann(core))
    encoded = case kind do
      :literal -> {kind, annotations, [], intern(:cerl.concrete(core)), :undefined, :undefined}
      :var -> {kind, annotations, [], intern(:cerl.var_name(core)), :undefined, :undefined}
      :opaque -> {kind, annotations, [], intern(elem(core, 2)), :undefined, :undefined}
      :map ->
        groups = flat_groups_etf(core)
        map_arg = flat_node_etf(:cerl.map_arg(core))
        {kind, annotations, groups, :undefined, map_arg, :cerl.is_c_map_pattern(core)}
      _ -> {kind, annotations, flat_groups_etf(core), :undefined, :undefined, :undefined}
    end
    id = Process.get(:core_nodes_count)
    Process.put(:core_nodes_count, id + 1)
    Process.put(:core_nodes, [encoded | Process.get(:core_nodes)])
    id
  end

  defp flat_groups_etf(core) do
    Enum.map(:cerl.subtrees(core), fn group ->
      group |> Enum.map(&flat_node_etf/1) |> ids()
    end)
  end

  defp flat_node_json(core) do
    kind = :cerl.type(core)
    base = %{kind: Atom.to_string(kind), annotations: intern(:cerl.get_ann(core))}
    encoded = case kind do
      :literal -> Map.merge(base, %{groups: [], value: intern(:cerl.concrete(core))})
      :var -> Map.merge(base, %{groups: [], value: intern(:cerl.var_name(core))})
      :opaque -> Map.merge(base, %{groups: [], value: intern(elem(core, 2))})
      :map -> Map.merge(base, %{groups: Enum.map(:cerl.subtrees(core), &Enum.map(&1, fn n -> flat_node_json(n) end)), map_arg: flat_node_json(:cerl.map_arg(core)), map_pattern: :cerl.is_c_map_pattern(core)})
      _ -> Map.put(base, :groups, Enum.map(:cerl.subtrees(core), &Enum.map(&1, fn n -> flat_node_json(n) end)))
    end
    id = Process.get(:core_nodes_count)
    Process.put(:core_nodes_count, id + 1)
    Process.put(:core_nodes, [encoded | Process.get(:core_nodes)])
    id
  end

  defp decode_flat_term(%{"kind" => "tuple", "items" => items}, terms), do: items |> Enum.map(&Map.fetch!(terms, &1)) |> List.to_tuple()
  defp decode_flat_term(%{"kind" => "list", "items" => items, "tail" => tail}, terms), do: Enum.reduce(Enum.reverse(items), Map.fetch!(terms, tail), fn item, acc -> [Map.fetch!(terms, item) | acc] end)
  defp decode_flat_term(%{"kind" => "map", "entries" => entries}, terms), do: Map.new(entries, fn [k, v] -> {Map.fetch!(terms, k), Map.fetch!(terms, v)} end)
  defp decode_flat_term(value, _terms), do: decode(value)

  defp rebuild_flat(node, nodes, terms) do
    annotations = Map.fetch!(terms, node["annotations"])
    children = Enum.map(node["groups"], &Enum.map(&1, fn id -> Map.fetch!(nodes, id) end))
    case node["kind"] do
      "literal" -> :cerl.ann_abstract(annotations, Map.fetch!(terms, node["value"]))
      "var" -> :cerl.ann_c_var(annotations, Map.fetch!(terms, node["value"]))
      "opaque" -> {:c_opaque, annotations, Map.fetch!(terms, node["value"])}
      "cons" ->
        [[head], [tail]] = children
        :cerl.ann_c_cons_skel(annotations, head, tail)
      "tuple" ->
        [items] = children
        :cerl.ann_c_tuple_skel(annotations, items)
      "map" ->
        [pairs] = children
        {:c_map, annotations, Map.fetch!(nodes, node["map_arg"]), pairs, node["map_pattern"]}
      kind -> :cerl.ann_make_tree(annotations, String.to_existing_atom(kind), children)
    end
  end

  def rebuild_nodes(exported) do
    terms = exported["terms"] |> Enum.with_index() |> Enum.reduce(%{}, fn {value, id}, acc -> Map.put(acc, id, decode_flat_term(value, acc)) end)
    nodes = exported["nodes"] |> Enum.with_index() |> Enum.reduce(%{}, fn {value, id}, acc -> Map.put(acc, id, rebuild_flat(value, acc, terms)) end)
    nodes
  end

  def rebuild_module(exported) do
    terms = exported["terms"] |> Enum.with_index() |> Enum.reduce(%{}, fn {value, id}, acc -> Map.put(acc, id, decode_flat_term(value, acc)) end)
    nodes = exported["nodes"] |> Enum.with_index() |> Enum.reduce(%{}, fn {value, id}, acc -> Map.put(acc, id, rebuild_flat(value, acc, terms)) end)
    {Map.fetch!(terms, exported["source_forms"]), Map.fetch!(nodes, exported["core"])}
  end

  def rebuild_terms(exported) do
    terms = exported["terms"] |> Enum.with_index() |> Enum.reduce(%{}, fn {value, id}, acc -> Map.put(acc, id, decode_flat_term(value, acc)) end)
    Map.fetch!(terms, exported["root"])
  end

  def capture_terms(value) do
    started = System.monotonic_time(:microsecond)
    Process.put(:core_terms_cache, %{})
    Process.put(:core_terms_count, 0)
    Process.put(:core_terms, [])
    root = intern(value)
    exported = %{root: root, terms: Enum.reverse(Process.get(:core_terms))}
    Enum.each([:core_terms_cache, :core_terms_count, :core_terms], &Process.delete/1)
    if System.get_env("RUST_DIALYZER_TRACE"),
      do: IO.puts(:stderr, "PERF arena.capture #{(System.monotonic_time(:microsecond) - started) / 1_000_000}")
    exported
  end

  def export_terms(value) do
    exported = capture_terms(value)
    started = System.monotonic_time(:microsecond)
    decoded = exported |> :json.encode() |> IO.iodata_to_binary() |> :json.decode() |> rebuild_terms()
    if :erlang.term_to_binary(decoded) !== :erlang.term_to_binary(value), do: raise("term arena roundtrip differs")
    if System.get_env("RUST_DIALYZER_TRACE"),
      do: IO.puts(:stderr, "PERF arena.roundtrip #{(System.monotonic_time(:microsecond) - started) / 1_000_000}")
    exported
  end

  defp constraint_variable({:var, _, name}) when (is_atom(name) or is_integer(name)) and name != :_, do: {:ok, name}
  defp constraint_variable({:ann_type, _, [_annotation, type]}), do: constraint_variable(type)
  defp constraint_variable(_), do: :unresolved

  defp bounded_variable_map({:type, _, :bounded_fun, [_fun, constraints]}) when is_list(constraints) do
    Enum.reduce_while(constraints, {:ok, %{}}, fn
      {:type, _, :constraint, [{:atom, _, :is_subtype}, [left, _right]]}, {:ok, map} ->
        case constraint_variable(left) do
          {:ok, name} -> {:cont, {:ok, Map.put(map, name, nil)}}
          :unresolved -> {:halt, :unresolved}
        end
      _, _ -> {:halt, :unresolved}
    end)
  end
  defp bounded_variable_map(_), do: :unresolved

  defp contract_variable_maps([], maps), do: maps
  defp contract_variable_maps([value | pending], maps) do
    maps = case bounded_variable_map(value) do
      {:ok, variables} -> Map.put(maps, intern(value), intern(variables))
      :unresolved -> maps
    end
    children = cond do
      is_tuple(value) -> Tuple.to_list(value)
      match?([_ | _], value) -> [hd(value), tl(value)]
      true -> []
    end
    contract_variable_maps(children ++ pending, maps)
  end

  def export_module(name, forms, core), do: name |> build_module(forms, core) |> verify_module(forms, core)

  # Builds the version 6 module record without the JSON roundtrip check.
  # `:etf` returns the compact cache tuple read by src/core/etf.rs instead.
  def build_module(name, forms, core, format \\ :json) do
    Process.put(:core_export_format, format)
    Process.put(:core_terms_cache, %{})
    Process.put(:core_terms_count, 0)
    Process.put(:core_terms, [])
    Process.put(:core_nodes_count, 0)
    Process.put(:core_nodes, [])
    source_forms = if format == :etf do
      source_forms = intern([])
      intern_source_atoms(forms)
      source_forms
    else
      intern(forms)
    end
    root = flat_node(core)
    variable_maps = contract_variable_maps(forms, %{}) |> Enum.sort() |> Enum.map(fn {form, map} -> [form, map] end)
    terms = Enum.reverse(Process.get(:core_terms))
    nodes = Enum.reverse(Process.get(:core_nodes))
    exported = case format do
      :json -> %{name: Atom.to_string(name), source_forms: source_forms, core: root,
                 contract_variable_maps: variable_maps, terms: terms, nodes: nodes}
      :etf -> {:core_module_v1, Atom.to_string(name), source_forms, root,
               ids(List.flatten(variable_maps)), terms, nodes}
    end
    Enum.each([:core_terms_cache, :core_terms_count, :core_terms, :core_nodes_count, :core_nodes, :core_export_format], &Process.delete/1)
    exported
  end

  def verify_module(exported, forms, core) do
    name = exported.name
    decoded = exported |> :json.encode() |> IO.iodata_to_binary() |> :json.decode()
    {decoded_forms, decoded_core} = rebuild_module(decoded)
    if :erlang.term_to_binary(decoded_forms) !== :erlang.term_to_binary(forms), do: raise("source term roundtrip differs: #{name}")
    if :erlang.term_to_binary(decoded_core) !== :erlang.term_to_binary(core), do: raise("Core roundtrip differs: #{name}")
    exported
  end

  def module(path) do
    {:ok, {name, [debug_info: {:debug_info_v1, backend, metadata}, abstract_code: {:raw_abstract_v1, forms}]}} =
      :beam_lib.chunks(String.to_charlist(path), [:debug_info, :abstract_code])
    {:ok, unclean} = backend.debug_info(:core_v1, name, metadata, @options)
    core = :dialyzer_clean_core.clean(unclean)
    {:ok, reference_core} = :dialyzer_utils.get_core_from_beam(String.to_charlist(path))
    if reference_core !== core, do: raise("OTP compiler profile differs: #{path}")
    export_module(name, forms, core)
  end

  def atom_layout(modules) do
    atoms = for module <- modules, %{kind: "atom", value: name} <- module.terms,
                do: String.to_existing_atom(name)
    CompilerAtomPrimitives.export(atoms ++ [:top, :external, :in, :out, :"$e", :"$v"],
                                  Base.encode16(:crypto.strong_rand_bytes(16)))
  end

  def run(beam_dir, output) do
    paths = Path.wildcard(Path.join(beam_dir, "*.beam"))
    if paths == [], do: raise("no BEAM files found in #{beam_dir}")
    modules = paths |> Enum.map(&module/1) |> Enum.sort_by(& &1.name)
    File.mkdir_p!(Path.dirname(output))
    File.write!(output, :json.encode(%{version: 6, profile: profile(), atom_layout: atom_layout(modules), modules: modules}))
    %{modules: length(modules), output: output}
  end
end
