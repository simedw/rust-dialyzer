Code.require_file("../core_export.exs",__DIR__)
Code.require_file("../type_codec.exs",__DIR__)
Code.require_file("../type_wire.exs",__DIR__)
source=File.read!(Path.join(__DIR__,"../pattern_probes/binder_oracle.exs"))|>String.split("fields=BinderOracle.load()")|>hd()|>String.split("defmodule BinderOracle")|>List.last()
Code.eval_string("defmodule BinderOracle"<>(source|>String.split(" def map_wire")|>hd())<>"end")
fields=BinderOracle.load()
defmodule GuardProducerOracle do
 def format(entry,args,types,state) do
  try do %{ok: apply(:rust_binder_native,entry,[args,types,state])|>List.to_string()}
  catch :error,:function_clause->%{error: %{kind: "error",class: "function_clause"}};c,r->raise "unclassified #{inspect({c,r})}" end
 end
end
pool=Enum.map(["any","none","unit","integer","atom","float","number","nil","map_any","tuple_any"],&%{"kind"=>&1})++[%{"kind"=>"atoms","values"=>["true","false"]},%{"kind"=>"nominal","name"=>%{"module"=>"other","name"=>"wrapped","arity"=>0,"opacity"=>"opaque"},"inner"=>%{"kind"=>"integer"}}]
for line<-IO.stream(:stdio,:line) do
 row=:json.decode(line)
 {mods,rs}=Enum.map_reduce(row["core"]["modules"],[],fn mod,acc->
  {forms,tree}=CoreExport.rebuild_module(mod)
  proof=CoreExport.export_module(String.to_atom(mod["name"]),forms,tree)|>:json.encode()|>IO.iodata_to_binary()|>:json.decode()
  nodes=CoreExport.rebuild_nodes(proof)
  records=Map.new(row["records"]||[],fn record->
   fs=Enum.with_index(record["fields"],fn t,i->{String.to_atom("field#{i}"),{:type,0,:any,[]},TypeCodec.type(t)}end)
   {{:record,String.to_atom(record["tag"])},{{~c"fixture",1},[{length(fs),fs}]}}
  end)
  state=:rust_binder_native.binder_state(:fixture,records,false)
  types=Enum.map(pool++(row["types_extra"]||[]),&TypeCodec.type/1)++[:cerl.c_atom(true)]
  outcomes=try do
   for {node,id}<-Enum.with_index(proof["nodes"]),node["kind"] in ["tuple","cons","map","literal","var"], {type,t}<-Enum.with_index(types), folded<-[false,true], entry<-[:format_args,:format_args_1] do
    arg=Map.fetch!(nodes,id);arg=if folded,do: :cerl.fold_literal(arg),else: arg
    %{node: id,type: t,folded: folded,entry: Atom.to_string(entry),result: GuardProducerOracle.format(entry,[arg],[type],state)}
   end
  after
   :dialyzer_callgraph.delete(BinderOracle.get(state,fields.state,:callgraph)); :dialyzer_codeserver.delete(BinderOracle.get(state,fields.state,:codeserver))
  end
  extra_state=:rust_binder_native.binder_state(:fixture,records,false)
  extra=try do
   ids=proof["nodes"]|>Enum.with_index()|>Enum.filter(fn {n,_}->n["kind"] in ["var","literal","tuple","cons"]end)|>Enum.map(&elem(&1,1))|>Enum.take(4)
   variants=[{[],[]},{[],[0]},{[0],[]},{[0],[0,1]},{[0,1],[0]},{[0,1],[0,3]},{[0,1],[length(types)-1,0]}]
   for {arg_offsets,type_ids}<-variants,length(ids)>=2,folded<-[false,true],entry<-[:format_args,:format_args_1] do
    args=Enum.map(arg_offsets,fn offset->node=Map.fetch!(nodes,Enum.at(ids,offset));if folded,do: :cerl.fold_literal(node),else: node end)
    arg_ids=Enum.map(arg_offsets,fn offset->%{node: Enum.at(ids,offset),folded: folded}end)
    %{args: arg_ids,types: type_ids,entry: Atom.to_string(entry),result: GuardProducerOracle.format(entry,args,Enum.map(type_ids,&Enum.at(types,&1)),extra_state)}
   end
  after
   :dialyzer_callgraph.delete(BinderOracle.get(extra_state,fields.state,:callgraph));:dialyzer_codeserver.delete(BinderOracle.get(extra_state,fields.state,:codeserver))
  end
  {proof,acc++[%{module: mod["name"],outcomes: outcomes++extra}]}
 end)
 IO.puts(:json.encode(%{id: row["id"],core: %{version: 5,profile: CoreExport.profile(),modules: mods},types: pool++(row["types_extra"]||[]),records: row["records"]||[],modules: rs}))
end
