[root,out]=System.argv()
true=Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(:code.which(:cerl_trees)))))=="F9C9D8C2DBCD7C2F170CA67124072A047D15B0DA5A9C4F81C270D111BFE6E727"
true=Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(:code.which(:cerl)))))=="0BE66918002098F37A5DDC7DE7AE2DF27A239E0889DA44F8EF08AF0C69B5073E"
Code.require_file(Path.join(root,"scripts/core_export.exs"))
lit=&:cerl.abstract/1
v=&:cerl.c_var/1
x=v.(:x);y=v.(:y);z=v.(:z)
pair=:cerl.c_map_pair_exact(lit.(:a),x)
segment=:cerl.c_bitstr(x,lit.(8),lit.(1),lit.(:integer),lit.([:unsigned,:big]))
clause=:cerl.c_clause([x],lit.(:true),x)
local=v.({:local,1});fun=:cerl.c_fun([y],y)
bodies=[lit.(%{a: {<<3::3>>, -0.0, 1_000_000_000_000_000_000_000}}),x,:cerl.c_values([x,y]),:cerl.c_binary([segment]),segment,:cerl.c_cons_skel(x,y),:cerl.c_tuple_skel([x,y]),:cerl.c_map_pattern([pair]),pair,:cerl.c_let([z],x,z),:cerl.c_seq(x,y),:cerl.c_apply(x,[y]),:cerl.c_call(lit.(:erlang),lit.(:hd),[x]),:cerl.c_primop(lit.(:match_fail),[x]),:cerl.c_case(x,[clause]),clause,:cerl.c_alias(z,x),fun,:cerl.c_receive([clause],lit.(0),y),:cerl.c_try(x,[z],z,[v.(:kind),v.(:reason),v.(:trace)],v.(:reason)),:cerl.c_catch(x),:cerl.c_letrec([{local,fun}],local),{:c_opaque,[],%{a: 1}}]
annotation=[{:label,72},:label,{:label,73},42,{:file,~c"source.erl"}]
bodies=bodies++Enum.map([x,lit.(:a),:cerl.c_tuple_skel([x,y]),fun,:cerl.c_map_pattern([pair])],&:cerl.set_ann(&1,annotation))
module=fn body,index ->
 name=String.to_atom("capture#{index}");variable=v.({:entry,3});fun=:cerl.c_fun([x,y,z],body)
 :cerl.ann_c_module(annotation,lit.(name),[:cerl.set_ann(variable,annotation)],[{:cerl.set_ann(lit.(:attr),annotation),:cerl.set_ann(lit.(%{a: 1}),annotation)}],[{variable,fun}])
end
bodies=bodies++[:cerl.update_c_map(:cerl.c_map_pattern([pair]),:cerl.set_ann(x,annotation),[pair])]
modules=Enum.with_index(bodies)|>Enum.map(fn {body,index}->module.(body,index)end)
modules=modules++[:cerl.c_module(lit.(:nested_capture),[local],[],[{local,fun}])]
rows=for initial<-[0,1000],{tree,index}<-Enum.with_index(modules) do
 name=:cerl.atom_val(:cerl.module_name(tree));exported=CoreExport.export_module(name,[],tree);{labeled,next}=:cerl_trees.label(tree,initial)
 %{id: "synthetic/#{index}/#{initial}",initial: initial,next_label: next,program: %{version: 6,profile: CoreExport.profile(),modules: [exported]},native: [CoreExport.export_terms([labeled])]}
end
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]))
IO.inspect(%{cases: length(rows)})
