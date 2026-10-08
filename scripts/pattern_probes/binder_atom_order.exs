# Each invocation is a new VM. Atom names are dynamic so source compilation does
# not allocate the two keys before this explicit allocation order.
Code.require_file("../core_export.exs",__DIR__)
[direction,case_path,native_path]=System.argv()
names=["binder_map_order_key_alpha_42b9","binder_map_order_key_beta_42b9"]
allocated=if direction=="ab",do: names,else: Enum.reverse(names)
Enum.each(allocated,&String.to_atom/1)
[a,b]=Enum.map(names,&String.to_existing_atom/1)
map=%{a=>42,b=>:bad};expr=:cerl.abstract(map)
root=:cerl.c_module(:cerl.c_atom(:fixture),[],[],[{:cerl.c_var({:fixture,0}),:cerl.c_fun([],:cerl.c_values([expr]))}])
mod=CoreExport.export_module(:fixture,[],root)
core=%{version: 5,profile: CoreExport.profile(),modules: [mod]}
row=%{id: "atom_order_#{direction}",core: core,types: [%{kind: "integer"}],reverse: false}
File.write!(case_path,[:json.encode(row),"\n"])
source=File.read!(Path.join(__DIR__,"binder_oracle.exs"))|>String.split("fields=BinderOracle.load()")|>hd()
Code.eval_string(source,[],file: Path.join(__DIR__,"binder_oracle.exs"))
File.write!(native_path,[:json.encode(BinderOracle.run(row|>:json.encode()|>IO.iodata_to_binary()|>:json.decode(),BinderOracle.load())),"\n"])
IO.puts(:json.encode(%{allocation: direction,to_list: Enum.map(:maps.to_list(map),fn {k,_}->Atom.to_string(k)end)}))
