defmodule NativeCoreTransport do
  defp list_parts([head | tail], acc), do: list_parts(tail, [head | acc])
  defp list_parts(tail, acc), do: {Enum.reverse(acc), tail}

  defp atom(atom) do
    seen = Process.get(:native_atoms_seen)
    unless MapSet.member?(seen, atom) do
      Process.put(:native_atoms_seen, MapSet.put(seen, atom))
      Process.put(:native_atoms, [atom | Process.get(:native_atoms)])
    end
  end

  defp source_atoms(value) when is_atom(value), do: atom(value)
  defp source_atoms(value) when is_tuple(value), do: value |> Tuple.to_list() |> Enum.each(&source_atoms/1)
  defp source_atoms([_ | _] = value) do
    {items, tail} = list_parts(value, [])
    Enum.each(items, &source_atoms/1)
    source_atoms(tail)
  end
  defp source_atoms(value) when is_map(value) do
    Enum.each(:maps.to_list(value), fn {key, item} -> source_atoms(key); source_atoms(item) end)
  end
  defp source_atoms(_), do: :ok

  defp iterator_keys(iterator, acc) do
    case :maps.next(iterator) do
      :none -> Enum.reverse(acc)
      {key, _value, next} -> iterator_keys(next, [key | acc])
    end
  end

  defp value(value, depth \\ 0)
  defp value(_value, depth) when depth >= 192, do: throw(:native_depth_fallback)
  defp value(value, _depth) when is_atom(value), do: atom(value)
  defp value(value, depth) when is_tuple(value), do: value |> Tuple.to_list() |> Enum.each(fn child -> value(child, depth + 1) end)
  defp value([_ | _] = value, depth) do
    {items, tail} = list_parts(value, [])
    Enum.each(items, fn child -> value(child, depth + 1) end)
    value(tail, depth + 1)
  end
  defp value(value, depth) when is_map(value) do
    pairs = :maps.to_list(value)
    positions = pairs |> Enum.with_index() |> Map.new(fn {{key, _}, index} -> {key, index} end)
    order = iterator_keys(:maps.iterator(value), []) |> Enum.map(&Map.fetch!(positions, &1))
    keys = Enum.map(pairs, &elem(&1, 0))
    Process.put(:native_root_maps, [{keys, order} | Process.get(:native_root_maps)])
    Enum.each(pairs, fn {key, item} -> value(key, depth + 1); value(item, depth + 1) end)
  end
  defp value(value, _depth) when is_integer(value) or is_float(value) or is_bitstring(value) or value == [], do: :ok

  defp root(value) do
    index = Process.get(:native_root_count)
    Process.put(:native_root_count, index + 1)
    Process.put(:native_root_maps, [])
    value(value)
    case Process.get(:native_root_maps) do
      [] -> :ok
      maps -> Process.put(:native_layouts, [{index, Enum.reverse(maps)} | Process.get(:native_layouts)])
    end
  end

  defp core(core) do
    root(:cerl.get_ann(core))
    case :cerl.type(core) do
      :literal -> root(:cerl.concrete(core))
      :var -> root(:cerl.var_name(core))
      :opaque -> root(elem(core, 2))
      :map ->
        Enum.each(:cerl.subtrees(core), &Enum.each(&1, fn child -> core(child) end))
        core(:cerl.map_arg(core))
      _ -> Enum.each(:cerl.subtrees(core), &Enum.each(&1, fn child -> core(child) end))
    end
    Process.put(:native_node_count, Process.get(:native_node_count) + 1)
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

  defp contracts([], rows), do: Enum.reverse(rows)
  defp contracts([value | pending], rows) do
    rows = case bounded_variable_map(value) do
      {:ok, variables} ->
        root(value)
        root(variables)
        [{value, variables} | rows]
      :unresolved -> rows
    end
    children = cond do
      is_tuple(value) -> Tuple.to_list(value)
      match?([_ | _], value) -> [hd(value), tl(value)]
      true -> []
    end
    contracts(children ++ pending, rows)
  end

  defp build_native(name, forms, cleaned) do
    Process.put(:native_atoms_seen, MapSet.new())
    Process.put(:native_atoms, [])
    Process.put(:native_root_count, 0)
    Process.put(:native_node_count, 0)
    Process.put(:native_root_maps, [])
    Process.put(:native_layouts, [])
    source_atoms(forms)
    source_atoms = Enum.reverse(Process.get(:native_atoms))
    core(cleaned)
    contracts = contracts(forms, [])
    record = {:native_core_module_v1, Atom.to_string(name), cleaned, source_atoms, contracts,
      Enum.reverse(Process.get(:native_layouts)), Process.get(:native_node_count), Process.get(:native_root_count)}
    atoms = Enum.reverse(Process.get(:native_atoms))
    Enum.each([:native_atoms_seen, :native_atoms, :native_root_count, :native_node_count,
      :native_root_maps, :native_layouts], &Process.delete/1)
    {record, atoms}
  end
  def build(name, forms, cleaned) do
    try do
      build_native(name, forms, cleaned)
    catch
      :native_depth_fallback ->
        {:core_module_v1, _, _, _, _, terms, _} = flat = CoreExport.build_module(name, forms, cleaned, :etf)
        {flat, for({:atom, atom} <- terms, do: atom)}
    after
      Enum.each([:native_atoms_seen, :native_atoms, :native_root_count, :native_node_count,
        :native_root_maps, :native_layouts], &Process.delete/1)
    end
  end

end
