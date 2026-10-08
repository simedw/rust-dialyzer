Code.require_file("core_export.exs",__DIR__)
defmodule CollectionContracts do
 def lookup(mfa,_server) do
  case Map.fetch(Process.get(:contracts),mfa) do
   {:ok,origin}->{:ok,{origin,:contract,:extra}}
   :error->:error
  end
 end
end
defmodule CollectionOracle do
 def rewrite({:call,a,{:remote,b,{:atom,c,:dialyzer_codeserver},{:atom,d,:lookup_mfa_contract}},args}),do: {:call,a,{:remote,b,{:atom,c,CollectionContracts},{:atom,d,:lookup}},Enum.map(args,&rewrite/1)}
 def rewrite(t) when is_tuple(t),do: t|>Tuple.to_list()|>Enum.map(&rewrite/1)|>List.to_tuple()
 def rewrite(t) when is_list(t),do: Enum.map(t,&rewrite/1)
 def rewrite(t),do: t
end
{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_succ_typings),[:abstract_code])
forms=Enum.map(forms,fn {:attribute,a,:module,_}->{:attribute,a,:module,:collection_native};form->CollectionOracle.rewrite(form)end)
{:ok,:collection_native,bytes}=:compile.forms(forms,[:binary,:export_all])
{:module,:collection_native}=:code.load_binary(:collection_native,~c"collection_native",bytes)
[mode,input,output]=System.argv()
if mode=="native" do
 File.open!(output,[:write],fn out->Enum.each(File.stream!(input),fn row->
  row=:json.decode(row);{warnings,contracts}=CoreExport.rebuild_terms(row["arena"]);Process.put(:contracts,Map.new(contracts))
  value=:collection_native.postprocess_warnings(warnings,:server)
  IO.binwrite(out,[:json.encode(%{id: row["id"],arena: CoreExport.export_terms(value)}),"\n"])
 end)end)
else
 n=Enum.to_list(File.stream!(input));r=Enum.to_list(File.stream!(output));if length(n)!=length(r),do: raise("collection output count changed")
 for {a,b}<-Enum.zip(n,r) do
  a=:json.decode(a);b=:json.decode(b)
  if a["id"]!=b["id"] or CoreExport.rebuild_terms(a["arena"])!==CoreExport.rebuild_terms(b["arena"]),do: raise("collection mismatch #{inspect(a["id"])}")
 end
 IO.puts(:json.encode(%{commands: length(n),matched: length(n)}))
end
