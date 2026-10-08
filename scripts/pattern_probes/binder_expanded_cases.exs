Code.require_file("../core_export.exs", __DIR__)
v=fn name->:cerl.c_var(name) end
x=v.(:X);y=v.(:Y);z=v.(:Z)
l=fn value->:cerl.abstract(value) end
seg=fn val,size,unit,type,flags->:cerl.c_bitstr(val,l.(size),l.(unit),l.(type),l.(flags)) end
patterns=[{:var,x},{:atom,l.(:tag)},{:integer,l.(42)},{:float,l.(1.25)},{:nil,l.([])},{:improper,l.([42|:tag])},{:string,l.(~c"AB")},{:bitsliteral,l.(<<1,2,3::size(3)>>)},{:empty_tuple,:cerl.c_tuple([])},{:tuple,:cerl.c_tuple([l.(:tag),x])},{:tuple_int,:cerl.c_tuple([x,l.(42)])},{:nested_tuple,:cerl.c_tuple([l.(:tag),:cerl.c_tuple([x,y])])},{:cons,:cerl.c_cons(x,y)},{:cons_nested,:cerl.c_cons(x,:cerl.c_cons(y,z))},{:alias,:cerl.c_alias(x,y)},{:alias_literal,:cerl.c_alias(x,l.(:tag))},{:alias_map,:cerl.c_alias(x,l.(%{key: 42}))},{:alias_tuple,:cerl.c_alias(x,:cerl.c_tuple([y,z]))},{:map_empty,:cerl.c_map_pattern([])},{:map,:cerl.c_map_pattern([:cerl.c_map_pair_exact(l.(:key),x)])},{:map_var_key,:cerl.c_map_pattern([:cerl.c_map_pair_exact(y,x)])},{:map_two,:cerl.c_map_pattern([:cerl.c_map_pair_exact(l.(:key),x),:cerl.c_map_pair_exact(l.(:other),y)])},{:map_duplicate,:cerl.c_map_pattern([:cerl.c_map_pair_exact(l.(:key),x),:cerl.c_map_pair_exact(l.(:key),l.(42))])},{:literal_map,l.(%{key: 42})},{:nested_literal_map,l.({:tag,[%{key: 42},%{other: []}]})},{:map_keymap,l.(%{%{key: 1}=>%{other: 42}})},{:values,:cerl.c_values([x,y])},{:record,:cerl.ann_c_tuple([:record],[l.(:tag),x])},{:record_badtag,:cerl.ann_c_tuple([:record],[l.(:tag),l.(:bad)])},{:record_varhead,:cerl.ann_c_tuple([:record],[x,y])},{:binary_empty,:cerl.c_binary([])}]
patterns=patterns ++ (for unit<-[0,1,2,8,64],size<-[0,1,8,16,128],signed<-[false,true],type<-[:integer,:float,:binary] do
 flags=if signed,do: [:signed,:little],else: [:unsigned,:big]
 {String.to_atom("binary_#{unit}_#{size}_#{signed}_#{type}"),:cerl.c_binary([seg.(x,size,unit,type,flags)])}
end) ++ (for type<-[:utf8,:utf16,:utf32],unit<-[:undefined,1] do
 {String.to_atom("binary_#{type}_#{unit}"),:cerl.c_binary([seg.(x,:undefined,unit,type,[])])}
end) ++ [{:binary_all,:cerl.c_binary([seg.(x,:all,8,:binary,[:unsigned,:big])])},{:binary_dynamic,:cerl.c_binary([:cerl.c_bitstr(x,y,l.(1),l.(:integer),l.([:signed,:big]))])},{:binary_two,:cerl.c_binary([seg.(x,8,1,:integer,[:unsigned,:big]),seg.(y,:all,8,:binary,[:unsigned,:big])])},{:binary_badsize,:cerl.c_binary([seg.(x,-1,1,:integer,[:unsigned,:big])])},{:binary_badatomsize,:cerl.c_binary([seg.(x,:badsize,1,:integer,[:unsigned,:big])])}]
base=[%{kind: "any"},%{kind: "none"},%{kind: "unit"},%{kind: "atom"},%{kind: "atoms",values: ["tag"]},%{kind: "atoms",values: ["key","other"]},%{kind: "integer"},%{kind: "integers",values: ["42"]},%{kind: "range",from: "0",to: "128"},%{kind: "float"},%{kind: "number"},%{kind: "tuple_any"},%{kind: "tuple",elements: [%{kind: "atoms",values: ["tag"]},%{kind: "integer"}]},%{kind: "tuple",elements: [%{kind: "atoms",values: ["tag"]},%{kind: "atoms",values: ["bad"]}]},%{kind: "union",values: [%{kind: "tuple",elements: [%{kind: "atoms",values: ["tag"]},%{kind: "integer"}]},%{kind: "tuple",elements: [%{kind: "atoms",values: ["other"]},%{kind: "atom"}]}]},%{kind: "nil"},%{kind: "list",elements: %{kind: "integer"},tail: %{kind: "nil"},nonempty: false},%{kind: "list",elements: %{kind: "atom"},tail: %{kind: "integer"},nonempty: true},%{kind: "map_any"},%{kind: "map",entries: [%{key: %{kind: "atoms",values: ["key"]},presence: "mandatory",value: %{kind: "integer"}}],default_key: %{kind: "none"},default_value: %{kind: "none"}},%{kind: "binary"},%{kind: "bitstring",unit: "0",base: "0"},%{kind: "bitstring",unit: "0",base: "8"},%{kind: "bitstring",unit: "0",base: "10"},%{kind: "bitstring",unit: "2",base: "1"},%{kind: "product",values: [%{kind: "integer"},%{kind: "atom"}]}]
nominals=for inner<-Enum.filter(base,&(&1.kind in ["integer","atom","tuple","list","map_any","binary"])),opacity<-["opaque","transparent"] do
 %{kind: "nominal",name: %{module: "other",name: "type",arity: 0,opacity: opacity},inner: inner}
end
types=base++nominals
File.mkdir_p!("data/cores")
for {name,pat}<-patterns do
 pat=:cerl.set_ann(pat,[1|:cerl.get_ann(pat)])
 root=:cerl.c_module(l.(:fixture),[],[],[{:cerl.c_var({:fixture,0}),:cerl.c_fun([],:cerl.c_values([pat]))}])
 mod=CoreExport.export_module(:fixture,[],root)
 core=%{version: 5,profile: CoreExport.profile(),modules: [mod]};path=Path.expand("data/cores/#{name}.json");File.write!(path,:json.encode(core))
 for {type,index}<-Enum.with_index(types),reverse<-[false,true] do
 row=%{id: "#{name}-#{index}-#{reverse}",core_json: path,types: [type],reverse: reverse,warnings: true,records: [%{tag: "tag",fields: [%{kind: "integer"}]}]}
 IO.puts(:json.encode(row))
 end
end
