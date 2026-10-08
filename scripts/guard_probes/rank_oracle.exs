Code.require_file("../type_codec.exs",__DIR__)
{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:erl_bif_types),[:abstract_code])
forms=Enum.map(forms,fn {:attribute,a,:module,_}->{:attribute,a,:module,:rust_bif_rank_native};f->f end)
{:ok,:rust_bif_rank_native,bin,_}=:compile.forms(forms,[:binary,:export_all,:return_warnings])
{:module,:rust_bif_rank_native}=:code.load_binary(:rust_bif_rank_native,~c"rank",bin)
for line<-IO.stream(:stdio,:line) do
 row=:json.decode(line)
 result=try do
  {min,max}=:rust_bif_rank_native.type_ranks(TypeCodec.type(row["type"]))
  %{ok: [min,max]}
 catch :error,:function_clause->%{error: %{kind: "error",class: "function_clause"}};c,r->raise "unclassified rank #{inspect({c,r,row})}"end
 IO.puts(:json.encode(Map.merge(row,result)))
end
