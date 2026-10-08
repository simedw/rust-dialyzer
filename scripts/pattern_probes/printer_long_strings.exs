Code.require_file("../core_export.exs",__DIR__)
for size<-[66660,66666,66667,100000,100001,200000],{chunk,name}<-[{"a","plain"},{"a ","space"},{"a\n\\","escape"}] do
 chars=String.duplicate(chunk,div(size,byte_size(chunk)))|>String.to_charlist();expr=:cerl.abstract(chars)
 tree=:cerl.c_module(:cerl.c_atom(:fixture),[],[],[{:cerl.c_var({:fixture,0}),:cerl.c_fun([],expr)}]);mod=CoreExport.export_module(:fixture,[],tree)
 [[_],_,_,[_name,fun_id]]=Enum.at(mod.nodes,mod.core).groups;[_,[body]]=Enum.at(mod.nodes,fun_id).groups
 IO.puts(:json.encode(%{id: "#{size}-#{name}",core: %{version: 5,profile: CoreExport.profile(),modules: [mod]},node: body,native: List.to_string(:cerl_prettypr.format(expr,[{:hook,:dialyzer_utils.pp_hook()},:noann,{:paper,100000},{:ribbon,100000}]))}))
end
