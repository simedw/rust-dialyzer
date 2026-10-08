[out,expected,requests]=System.argv()
for line<-File.stream!(requests) do
 row=:json.decode(line); id=row["id"]
 rp=Path.join(out,id<>".iplt");np=Path.join(expected,id<>".native.iplt")
 r=:erlang.binary_to_term(File.read!(rp));n=:erlang.binary_to_term(File.read!(np))
 true=tuple_size(r)==10;true=elem(r,0)==:ifile_plt
 for i<-[1,2,9],do: true=elem(r,i)===elem(n,i)
 for i<-[3,4,5,7],do: true=:erlang.binary_to_term(elem(r,i))===:erlang.binary_to_term(elem(n,i))
 true=Map.new(:erlang.binary_to_term(elem(r,6)))===Map.new(:erlang.binary_to_term(elem(n,6)))
 rd=:erlang.binary_to_term(elem(r,8));nd=:erlang.binary_to_term(elem(n,8))
 true=elem(rd,0)==:incremental_data
 true=Map.new(:dict.to_list(elem(rd,1)))===Map.new(:dict.to_list(elem(nd,1)))
 for i<-[2,3],do: true=elem(rd,i)===elem(nd,i)
 {ra,ri}=:dialyzer_iplt.plt_and_info_from_file(String.to_charlist(rp))
 {na,ni}=:dialyzer_iplt.plt_and_info_from_file(String.to_charlist(np))
 true=elem(ri,1)===elem(ni,1)
 for i<-[1,2,3,4],do: true=Map.new(:ets.tab2list(elem(ra,i)))===Map.new(:ets.tab2list(elem(na,i)))
 true=MapSet.new(:ets.tab2list(elem(ra,5)))===MapSet.new(:ets.tab2list(elem(na,5)))
 :dialyzer_plt.delete(ra);:dialyzer_plt.delete(na)
 IO.puts(:json.encode(%{id: id,container: true,native_reader: true,callback_reload: true,cache: true,types_logical: true}))
end
