import Bitwise

Code.require_file("core_export.exs", __DIR__)

terms = [:example, "example", :nil, [], {}, 1 <<< 256, -(1 <<< 256), -0.0,
         <<1::size(1)>>, <<5::size(3)>>, <<255, 7::size(3)>>,
         [:a, 1 | :tail], %{"a" => <<1, 2>>, a: {:nested, [1, 2]}}]
large_map = Map.new(1..80, fn key -> {{:key, key}, <<key::16>>} end)
terms = terms ++ [large_map, {:nested, [large_map]}]
exported = CoreExport.export_terms(large_map) |> :json.encode() |> IO.iodata_to_binary() |> :json.decode()
keys = exported["terms"] |> Enum.at(exported["root"]) |> Map.fetch!("entries") |> Enum.map(fn [key, _] ->
  CoreExport.rebuild_terms(Map.put(exported, "root", key))
end)
expected_keys = :maps.to_list(large_map) |> Enum.map(&elem(&1, 0))
if expected_keys == Enum.sort(expected_keys), do: raise("large-map probe must exercise unsorted native iteration")
if keys !== expected_keys, do: raise("native literal map iteration order lost")
iterator_keys = Stream.unfold(:maps.iterator(large_map), fn iterator ->
  case :maps.next(iterator) do
    :none -> nil
    {key, _value, next} -> {key, next}
  end
end) |> Enum.to_list()
record = Enum.at(exported["terms"], exported["root"])
actual_iterator_keys = Enum.map(record["iterator_order"], &Enum.at(keys, &1))
if actual_iterator_keys !== iterator_keys, do: raise("native map iterator order lost")
if iterator_keys === expected_keys, do: raise("large-map probe must exercise distinct native orders")


Enum.each(terms, fn value ->
  exported = CoreExport.export_module(:example, [value], :cerl.c_module(:cerl.c_atom(:example), [], []))
  decoded = exported |> :json.encode() |> IO.iodata_to_binary() |> :json.decode() |> CoreExport.rebuild_module() |> elem(0) |> hd()
  if :erlang.term_to_binary(value) != :erlang.term_to_binary(decoded), do: raise("literal fidelity failed: #{inspect(value)}")
end)

v = :cerl.c_var(:value)
f = :cerl.c_var({:f, 1})
atom = :cerl.c_atom(:ok)
clause = :cerl.c_clause([v], :cerl.c_atom(true), v)
fun = :cerl.c_fun([v], v)
pair = :cerl.ann_c_map_pair([], :cerl.c_atom(:assoc), atom, v)
nodes = [atom, v, :cerl.c_values([v]), :cerl.c_binary([]),
         :cerl.c_bitstr(v, :cerl.c_int(8), :cerl.c_int(1), :cerl.c_atom(:integer), :cerl.abstract([:unsigned, :big])),
         :cerl.c_cons_skel(atom, :cerl.c_nil()), :cerl.c_tuple_skel([atom]),
         :cerl.c_map(v, [pair]), :cerl.c_map_pattern([:cerl.c_map_pair_exact(atom, v)]), pair,
         :cerl.c_let([v], atom, v), :cerl.c_seq(atom, v), :cerl.c_apply(f, [v]),
         :cerl.c_call(:cerl.c_atom(:erlang), :cerl.c_atom(:abs), [v]),
         :cerl.c_primop(:cerl.c_atom(:match_fail), [atom]), :cerl.c_case(v, [clause]),
         clause, :cerl.c_alias(v, atom), fun, :cerl.c_receive([clause], :cerl.c_int(0), atom),
         :cerl.c_try(v, [v], v, [:cerl.c_var(:class), :cerl.c_var(:reason), :cerl.c_var(:stack)], atom),
         :cerl.c_catch(v), :cerl.c_letrec([{f, fun}], atom),
         :cerl.c_module(:cerl.c_atom(:example), [f], [{f, fun}]), {:c_opaque, [], :compiler_test}]

Enum.each(nodes, fn node ->
  node = :cerl.set_ann(node, [{42, 7}, {:file, ~c"literal.ex"}, :compiler_generated])
  exported = CoreExport.export_module(:example, [], node)
  rebuilt = exported |> :json.encode() |> IO.iodata_to_binary() |> :json.decode() |> CoreExport.rebuild_module() |> elem(1)
  if :erlang.term_to_binary(rebuilt) != :erlang.term_to_binary(node), do: raise("node fidelity failed: #{:cerl.type(node)}")
end)
IO.puts("PASS: #{length(terms)} literal edge cases; #{length(nodes)} node cases; #{nodes |> Enum.map(&:cerl.type/1) |> Enum.uniq() |> length()} Core kinds")
