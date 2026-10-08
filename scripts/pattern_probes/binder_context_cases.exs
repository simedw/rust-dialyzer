Code.require_file("../core_export.exs", __DIR__)
x=:cerl.c_var(:X);y=:cerl.c_var(:Y);z=:cerl.c_var(:Z);l=fn t->:cerl.abstract(t)end
inputs=[{:alias_chain,[:cerl.c_alias(y,z),:cerl.c_alias(x,y)],[]},{:alias_self,[:cerl.c_alias(y,z),:cerl.c_alias(y,y)],[]},{:letrec,[x],[{:X,%{kind: "function",domain: [%{kind: "integer"}],range: %{kind: "atom"}}}]},{:literal_hamt,[l.(Map.new(1..80,fn i->{i,if(rem(i,3)==0,do: %{v: i},else: {:tag,i})}end))],[]}]
for {name,pats,letrec}<-inputs do
 pats=Enum.map(pats,&:cerl.set_ann(&1,[1|:cerl.get_ann(&1)]))
 tree=:cerl.c_module(l.(:fixture),[],[],[{:cerl.c_var({:fixture,3}),:cerl.c_fun([x,y,z],:cerl.c_values(pats))}]);{labeled,_}=:cerl_trees.label(tree,0)
 labels=:cerl_trees.fold(fn node,acc->if :cerl.is_c_var(node),do: Map.put(acc,:cerl.var_name(node),:cerl_trees.get_label(node)),else: acc end,%{},labeled)
 core=%{version: 5,profile: CoreExport.profile(),modules: [CoreExport.export_module(:fixture,[],tree)]};path=Path.expand("data/cores/context_#{name}.json");File.write!(path,:json.encode(core))
 for type<-[%{kind: "any"},%{kind: "integer"},%{kind: "none"},%{kind: "unit"},%{kind: "map_any"}],reverse<-[false,true] do
  IO.puts(:json.encode(%{id: "context_#{name}_#{type.kind}_#{reverse}",core_json: path,types: List.duplicate(type,length(pats)),reverse: reverse,letrec: Enum.map(letrec,fn {v,t}->[Map.fetch!(labels,v),t]end)}))
 end
end
