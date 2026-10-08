Code.require_file("core_export.exs",__DIR__)
[out,mode]=System.argv();File.mkdir_p!(out)
names=Enum.map(0..79,&("contract_provenance_fresh#{&1}"))
Enum.each(if(mode=="forward",do: names,else: Enum.reverse(names)),&String.to_atom/1)
wire=fn v when is_atom(v)->["atom",Atom.to_string(v)];v when is_integer(v)->["integer",Integer.to_string(v)]end
for count<-[0,1,2,3,31,32,33,64,80], family<-[:atoms,:integers,:mixed], annotation<-[false,true] do
 keys=for i<-Enum.to_list(0..(count-1)//1),do: (case family do :integers->i;:mixed when rem(i,2)==0->i;_->String.to_atom(Enum.at(names,i))end)
 left=fn key->v={:var,0,key};if annotation,do: {:ann_type,0,[{:var,0,:Ignored},v]},else: v end
 constraints=Enum.map(keys++Enum.take(keys,2),fn key->{:type,0,:constraint,[{:atom,0,:is_subtype},[left.(key),{:type,0,:integer,[]}]]}end)
 bounded={:type,0,:bounded_fun,[{:type,0,:fun,[{:type,0,:product,[]},{:type,0,:integer,[]}]},constraints]}
 forms=[{:attribute,0,:spec,{{:entry,0},[bounded]}}]
 entry=:cerl.c_var({:entry,0});tree=:cerl.c_module(:cerl.c_atom(:contract_map),[entry],[],[{entry,:cerl.c_fun([],:cerl.c_atom(:ok))}])
 exported=CoreExport.export_module(:contract_map,forms,tree)
 id="#{mode}/#{count}/#{family}/#{annotation}";path=Path.expand(Path.join(out,Base.encode16(:crypto.hash(:sha256,id),case: :lower)<>".json"))
 File.write!(path,:json.encode(%{version: 6,profile: CoreExport.profile(),modules: [exported]}))
 variables=Enum.reduce(keys++Enum.take(keys,2),%{},fn k,m->Map.put(m,k,:erl_types.t_integer())end)
 fold=:maps.fold(fn k,_,acc->[wire.(k)|acc]end,[],variables)
 iterator=Stream.unfold(:maps.iterator(variables),fn i->case :maps.next(i)do :none->nil;{key,_,next}->{wire.(key),next}end end)|>Enum.to_list()
 IO.puts(:json.encode(%{id: id,core_json: path,maps: [%{iterator: iterator,fold: fold}]}))
end
