defmodule ExactTermComparison do
  def difference(a, b, _path) when a === b, do: nil
  def difference(a, b, path) when is_tuple(a) and is_tuple(b) and tuple_size(a) == tuple_size(b) do
    Enum.find_value(0..(tuple_size(a)-1), fn index -> difference(elem(a,index), elem(b,index), path <> "/tuple[#{index}]") end)
  end
  def difference([ah | at], [bh | bt], path), do: difference(ah,bh,path <> "/head") || difference(at,bt,path <> "/tail")
  def difference(a, b, path) when is_map(a) and is_map(b) and map_size(a) == map_size(b) do
    Enum.find_value(a, fn {key,value} -> if Map.has_key?(b,key), do: difference(value,Map.fetch!(b,key),path <> "/map[#{inspect(key)}]"), else: {path, {:missing_key,key}, :missing} end)
  end
  def difference(a,b,path), do: {path,a,b}
end
