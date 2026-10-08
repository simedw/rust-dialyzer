Code.require_file("type_codec.exs",__DIR__)
Code.require_file("type_wire.exs",__DIR__)
defmodule DataflowMapOracle do
  def load do
    {:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_dataflow),[:abstract_code])
    {:attribute,_,:record,{:map,fields}}=Enum.find(forms,&match?({:attribute,_,:record,{:map,_}},&1))
    fields=Enum.map(fields,fn f->f=if elem(f,0)==:typed_record_field,do: elem(f,1),else: f;elem(elem(f,2),2) end)
    forms=Enum.map(forms,fn {:attribute,a,:module,_}->{:attribute,a,:module,:rust_dataflow_map_native}; f->f end)
    {:ok,:rust_dataflow_map_native,binary,_}=:compile.forms(forms,[:binary,:export_all,:return_warnings])
    {:module,:rust_dataflow_map_native}=:code.load_binary(:rust_dataflow_map_native,~c"dataflow_map",binary)
    fields
  end
  def var(label),do: :cerl.ann_c_var([{:label,label}],String.to_atom("v#{label}"))
  def operations(map,steps) do
    Enum.reduce(steps,map,fn step,map->
      case step["operation"] do
        "enter"->:rust_dataflow_map_native.enter_type(var(step["key"]),TypeCodec.type(step["type"]),map)
        "literal"->:rust_dataflow_map_native.store_map(step["key"],TypeCodec.type(step["type"]),map)
        "alias"->:rust_dataflow_map_native.enter_subst(var(step["key"]),var(step["value"]),map)
        "join"->
          base=:rust_dataflow_map_native.join_maps_begin(map)
          branches=Enum.map(step["branches"],&operations(base,&1))
          :rust_dataflow_map_native.join_maps_end(branches,base)
      end
    end)
  end
  def get(map,fields,name),do: elem(map,Enum.find_index(fields,&(&1==name))+1)
end
fields=DataflowMapOracle.load()
for line<-IO.stream(:stdio,:line) do
  row=:json.decode(line)
  map=DataflowMapOracle.operations(:rust_dataflow_map_native.map__new(),row["steps"])
  types=Map.new(DataflowMapOracle.get(map,fields,:map),fn {key,type}->{Integer.to_string(key),NativeTypeWire.encode(type)} end)
  substitutions=Map.new(DataflowMapOracle.get(map,fields,:subst),fn {key,node}->{Integer.to_string(key),if(is_integer(node),do: node,else: :cerl_trees.get_label(node))} end)
  lookups=Enum.map(0..15,fn key->:rust_dataflow_map_native.lookup_type(DataflowMapOracle.var(key),map)|>NativeTypeWire.encode() end)
  IO.puts(:json.encode(%{id: row["id"],types: types,substitutions: substitutions,modified: DataflowMapOracle.get(map,fields,:modified),lookup: lookups}))
end
