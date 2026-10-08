[out]=System.argv()
File.mkdir_p!(out)
source=File.read!("/private/tmp/rust-dialyzer-incremental-container-proof/proof/empty.native.iplt")|>:erlang.binary_to_term()
cache=[none: :none,map: %{},scalar: :bad_cache,improper: %{m: [{:warn,{~c"x",0,:m},:reason}|:tail]}]
deps=[proper: :dict.new(),scalar: :bad_deps,improper: [:x|:tail],malformed_value: :dict.from_list([{:m,:bad_value}])]
for {cn,c}<-cache,{dn,d}<-deps,encoding<-[:binary,:legacy] do
 id="#{cn}-#{dn}-#{encoding}"
 data={:incremental_data,d,c,:none}
 field=if encoding==:binary,do: :erlang.term_to_binary(data,[:compressed]),else: data
 raw=put_elem(source,8,field); path=Path.join(out,id<>".iplt")
 File.write!(path,:erlang.term_to_binary(raw))
 original=if is_binary(elem(raw,8)),do: :erlang.binary_to_term(elem(raw,8)),else: elem(raw,8)
 fields=[elem(raw,1),elem(raw,2),elem(raw,9),elem(original,1),elem(original,2),elem(original,3)]
 IO.puts(:json.encode(%{id: id,path: path,expected: Enum.map(fields,fn f->Base.encode16(:erlang.term_to_binary(f),case: :lower)end)}))
end
