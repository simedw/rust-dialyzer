defmodule SourceCaptureWire do
  def encode(values,reference) do
    {root,{terms,_}}=node(values,{[],0},reference)
    %{root: root,terms: Enum.reverse(terms)}
  end
  defp node(value,state,reference) do
    {term,{terms,count}}=cond do
      is_reference(value)->id=reference.(value);{%{kind: "runtime",runtime_kind: "reference",scope: id.scope,object: id.object},state}
      is_function(value)->
        info=Map.new(:erlang.fun_info(value))
        {environment,state}=Enum.map_reduce(info.env,state,&node(&1,&2,reference))
        {%{kind: "function",flavor: Atom.to_string(info.type),module: Atom.to_string(info.module),name: Atom.to_string(info.name),arity: info.arity,new_index: Map.get(info,:new_index,0),uniq: Map.get(info,:uniq,0),environment: environment},state}
      is_atom(value)->{%{kind: "atom",value: Atom.to_string(value)},state}
      is_integer(value)->{%{kind: "integer",value: Integer.to_string(value)},state}
      is_float(value)->{%{kind: "float",value: Base.encode16(<<value::float-size(64)>>)},state}
      is_binary(value)->{%{kind: "binary",value: Base.encode64(value)},state}
      is_bitstring(value)->bits=bit_size(value);pad=rem(8-rem(bits,8),8);{%{kind: "bitstring",bytes: Base.encode64(<<value::bitstring,0::size(pad)>>),bits: bits},state}
      is_tuple(value)->{items,state}=Enum.map_reduce(Tuple.to_list(value),state,&node(&1,&2,reference));{%{kind: "tuple",items: items},state}
      is_map(value)->{entries,state}=Enum.map_reduce(Map.to_list(value),state,fn {key,val},state->{key,state}=node(key,state,reference);{val,state}=node(val,state,reference);{[key,val],state}end);{%{kind: "map",entries: entries},state}
      value==[]->{%{kind: "nil"},state}
      true->{items,tail}=prefix(value,[]);{items,state}=Enum.map_reduce(items,state,&node(&1,&2,reference));{tail,state}=node(tail,state,reference);{%{kind: "list",items: items,tail: tail},state}
    end
    {count,{[term|terms],count+1}}
  end
  defp prefix([head|tail],items),do: prefix(tail,[head|items])
  defp prefix(tail,items),do: {Enum.reverse(items),tail}
end
