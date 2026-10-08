defmodule ConstructorOracle do
 def endpoint("neg_inf"),do: :neg_inf
 def endpoint("pos_inf"),do: :pos_inf
 def endpoint(s),do: String.to_integer(s)
 def term(%{"term"=>"integer","value"=>s}),do: String.to_integer(s)
 def term(%{"term"=>"atom","value"=>s}),do: String.to_atom(s)
 def term(%{"term"=>"tuple","values"=>v}),do: List.to_tuple(Enum.map(v,&term/1))
 def term(%{"term"=>"list","values"=>v}),do: Enum.map(v,&term/1)
 def term(%{"term"=>"nil"}),do: []
 def check(c,r) do
  try do
   native=case c["op"] do
    "range"->:erl_types.t_from_range(endpoint(c["a"]),endpoint(c["b"]))
    "bitstring"->:erl_types.t_bitstr(String.to_integer(c["a"]),String.to_integer(c["b"]))
   end
   native===term(r["value"]) && (c["op"]!="bitstring" || r["canonical"]==(String.to_integer(c["a"])>=0 && String.to_integer(c["b"])>=0))
  catch :error,:function_clause->r["error"]=="function_clause" end
 end
end
[cases,results]=System.argv()
rows=Enum.zip(File.stream!(cases),File.stream!(results))
fail=Enum.filter(rows,fn {c,r}->!ConstructorOracle.check(:json.decode(c),:json.decode(r)) end)
IO.puts(:json.encode(%{cases: length(rows),failures: length(fail),first_failure: List.first(fail)}))
if fail != [], do: System.halt(1)
