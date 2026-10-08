Code.require_file("forms_runtime.exs",__DIR__)
defmodule Oracle do
 def term(a,id) do
  if Process.get(:metadata_arena)!=a do
   Process.put(:metadata_arena,a)
   Process.put(:metadata_decoded,FormsOracle.decode(a))
  end
  :array.get(id,Process.get(:metadata_decoded))
 end
 def expected(c) do
 if c["op"] in ["cache_new","var_table_new"] do
  case c["op"] do "cache_new"->:erl_types.cache__new();"var_table_new"->:erl_types.var_table__new() end
 else
 a=c["arena"];v=term(a,a["root"])
 case c["op"] do
 "deps"->:erl_types.type_form_to_remote_modules(v)
 "table_deps"->:erl_types.module_type_deps_of_type_defs(v)
 "vars"->:erl_types.t_var_names(v)
 "lookup"->:erl_types.lookup_record(String.to_atom(c["tag"]),term(a,c["arity"]),v)
 "defined"->:erl_types.type_is_defined(term(a,c["kind"]),term(a,c["name"]),term(a,c["arity"]),v)
 end
 end
 end
 def compare(c,r) do
 try do
 e=expected(c)
 a=c["arena"]
 got=case c["op"] do
 "cache_new"->term(r["ok"],r["ok"]["root"])
 "var_table_new"->term(r["ok"],r["ok"]["root"])
 "vars"->Enum.map(r["ok"]||[],&term(a,&1))
 "lookup"->if r["ok"] in [nil,:null],do: :error,else: {:ok,term(a,r["ok"])}
 "deps"->Enum.map(r["ok"]||[],&String.to_atom/1)
 "table_deps"->Enum.map(r["ok"]||[],&String.to_atom/1)
 _->r["ok"]
 end
 is_nil(r["error"]) && e==got
 catch
 :error,:function_clause->r["error"]=="FunctionClause"
 :error,{:badmatch,payload}->r["error"]=="BadMatchModules" && payload==Enum.map(r["payload"],&String.to_atom/1)
 k,e->IO.puts(:stderr,inspect({k,e}));false
 end
 end
end
[p,r]=System.argv()
{count,diffs}=Enum.zip(File.stream!(p),File.stream!(r))|>Enum.with_index()|>Enum.reduce({0,[]},fn {{c,r},i},{count,diffs}->c=:json.decode(c);r=:json.decode(r);if Oracle.compare(c,r),do: {count+1,diffs},else: {count+1,[i|diffs]}end)
report=%{count: count,diff_count: length(diffs),diffs: Enum.take(Enum.reverse(diffs),20)}
File.write!(r<>".comparison.json",:json.encode(report));IO.puts(:json.encode(report));if diffs != [],do: raise("metadata parity mismatch")
