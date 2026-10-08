Code.require_file("../core_export.exs",__DIR__)
defmodule FormIoCapture do
 def capture(c) do
  {terms,orders}=c["arena"]["terms"]|>Enum.with_index()|>Enum.reduce({%{},[]},fn {x,id},{terms,orders}->
   v=case x["kind"] do
    "tuple"->x["items"]|>Enum.map(&Map.fetch!(terms,&1))|>List.to_tuple()
    "list"->Enum.reduce(Enum.reverse(x["items"]),Map.fetch!(terms,x["tail"]),fn i,t->[Map.fetch!(terms,i)|t]end)
    "map"->Map.new(x["entries"],fn [k,v]->{Map.fetch!(terms,k),Map.fetch!(terms,v)}end)
    _->CoreExport.decode(x)
   end
   order=if is_map(v) do
    indexes=x["entries"]|>Enum.with_index()|>Map.new(fn {[k,_],i}->{Map.fetch!(terms,k),i}end)
    Enum.map(iterator(:maps.iterator(v)),&Map.fetch!(indexes,&1))
   else nil end
   {Map.put(terms,id,v),if(is_nil(order),do: orders,else: [%{id: id,order: order}|orders])}
  end)
  v=Map.fetch!(terms,c["arena"]["root"])
  expected=:io_lib.write(v,[{:encoding,String.to_atom(c["encoding"])}])
  c|>Map.put("map_orders",orders)|>Map.put("native_expected",CoreExport.term(expected))
 end
 def iterator(it) do
  case :maps.next(it) do {k,_,n}->[k|iterator(n)];:none->[] end
 end
end
[input,output]=System.argv();File.open!(output,[:write],fn io->Enum.each(File.stream!(input),fn s->IO.binwrite(io,[:json.encode(FormIoCapture.capture(:json.decode(s))),"\n"])end)end)
