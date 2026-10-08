Code.require_file("core_export.exs",__DIR__)
[out]=System.argv();File.mkdir_p!(out)
lit=&:cerl.abstract/1
catalog=:json.decode(File.read!("corpus/bifs.json"))["entries"]
known_calls=Enum.map(catalog,fn entry->m=entry["mfa"];:cerl.c_call(lit.(String.to_atom(m["module"])),lit.(String.to_atom(m["function"])),List.duplicate(lit.(:ok),m["arity"]))end)
for {count,index}<-Enum.with_index([0,1,2,15,16,17,79,80,81,95,96,97,159,160,161,255]) do
 name=String.to_atom("graph_fixture#{index}");entry=:cerl.c_var({:entry,1});x=:cerl.c_var(:x)
 remote=fn n->:cerl.c_call(lit.(:missing),lit.(String.to_atom("callee#{rem(n,5)}")),[x])end
 nested=for n<-Enum.to_list(1..count//1),do: :cerl.c_fun([], :cerl.c_tuple_skel([remote.(n),:cerl.c_fun([],remote.(n))]))
 make_fun=Enum.map([0,1,255],fn a->:cerl.c_call(lit.(:erlang),lit.(:make_fun),[lit.(:missing),lit.(:target),lit.(a)])end)
 dynamic=[:cerl.c_call(x,lit.(:target),[]),:cerl.c_call(lit.(:missing),x,[]),:cerl.c_call(lit.(:erlang),lit.(:make_fun),[x,lit.(:target),lit.(1)])]
 body=:cerl.c_tuple_skel(nested++known_calls++make_fun++dynamic++[remote.(0),remote.(0)])
 tree=:cerl.c_module(lit.(name),[entry],[],[{entry,:cerl.c_fun([x],body)}])
 exported=CoreExport.export_module(name,[],tree);path=Path.expand(Path.join(out,"#{index}.json"))
 File.write!(path,:json.encode(%{version: 5,profile: CoreExport.profile(),modules: [exported]}))
 IO.puts(:json.encode(%{id: "source_graph/#{index}",core_json: path}))
end

for {arity,index} <- Enum.with_index([-1,256,-18446744073709551617,18446744073709551617]) do
 name=String.to_atom("invalid_graph_arity#{index}");entry=:cerl.c_var({:entry,0})
 body=:cerl.c_call(lit.(:erlang),lit.(:make_fun),[lit.(:missing),lit.(:target),lit.(arity)])
 tree=:cerl.c_module(lit.(name),[entry],[],[{entry,:cerl.c_fun([],body)}])
 path=Path.expand(Path.join(out,"invalid#{index}.json"));exported=CoreExport.export_module(name,[],tree)
 File.write!(path,:json.encode(%{version: 5,profile: CoreExport.profile(),modules: [exported]}))
 IO.puts(:json.encode(%{id: "source_graph/invalid_arity#{index}",core_json: path}))
end
