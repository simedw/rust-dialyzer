Code.require_file("../core_export.exs", __DIR__)
v=fn name->:cerl.c_var(name) end
x=v.(:X);y=v.(:Y)
patterns=[{:var,x},{:atom,:cerl.c_atom(:tag)},{:empty_tuple,:cerl.c_tuple([])},{:tuple,:cerl.c_tuple([:cerl.c_atom(:tag),x])},{:cons,:cerl.c_cons(x,y)},{:alias,:cerl.c_alias(x,y)},{:alias_literal,:cerl.c_alias(x,:cerl.c_atom(:tag))},{:map,:cerl.c_map_pattern([:cerl.c_map_pair_exact(:cerl.c_atom(:key),x)])},{:literal_map,:cerl.abstract(%{key: 42})},{:nested_literal_map,:cerl.abstract({:tag,[%{key: 42}]})},{:values,:cerl.c_values([x,y])},{:record,:cerl.ann_c_tuple([:record],[:cerl.c_atom(:tag),x])},{:binary,:cerl.c_binary([:cerl.c_bitstr(x,:cerl.c_int(8),:cerl.c_int(1),:cerl.c_atom(:integer),:cerl.abstract([:unsigned,:big]))])}]
types=[%{kind: "any"},%{kind: "none"},%{kind: "unit"},%{kind: "atom"},%{kind: "atoms",values: ["tag"]},%{kind: "integer"},%{kind: "integers",values: ["42"]},%{kind: "tuple_any"},%{kind: "tuple",elements: [%{kind: "atoms",values: ["tag"]},%{kind: "integer"}]},%{kind: "nil"},%{kind: "list",elements: %{kind: "integer"},tail: %{kind: "nil"},nonempty: false},%{kind: "map_any"},%{kind: "map",entries: [%{key: %{kind: "atoms",values: ["key"]},presence: "mandatory",value: %{kind: "integer"}}],default_key: %{kind: "none"},default_value: %{kind: "none"}},%{kind: "binary"},%{kind: "bitstring",unit: "0",base: "8"},%{kind: "product",values: [%{kind: "integer"},%{kind: "atom"}]}]
for {name,pat}<-patterns,{type,index}<-Enum.with_index(types),reverse<-[false,true] do
 root=:cerl.c_module(:cerl.c_atom(:fixture),[],[],[{:cerl.c_var({:fixture,0}),:cerl.c_fun([],:cerl.c_values([pat]))}])
 mod=CoreExport.export_module(:fixture,[],root)
 row=%{id: "#{name}-#{index}-#{reverse}",core: %{version: 5,profile: CoreExport.profile(),modules: [mod]},types: [type],reverse: reverse,records: [%{tag: "tag",fields: [%{kind: "integer"}]}]}
 IO.puts(:json.encode(row))
end
