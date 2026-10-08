Code.require_file("../core_export.exs", __DIR__)
{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:erl_pp),[:abstract_code])
forms=Enum.map(forms,fn {:attribute,a,:module,_}->{:attribute,a,:module,:syntax_native_pp};f->f end)
{:ok,:syntax_native_pp,binary,_}=:compile.forms(forms,[:binary,:export_all,:return_errors,:return_warnings])
{:module,:syntax_native_pp}=:code.load_binary(:syntax_native_pp,~c"syntax_native_pp",binary)
opts=[quote_singleton_atom_types: true]
for line<-File.stream!(hd(System.argv())) do
 row=:json.decode(line)
 if row["parse"]["status"]=="parsed" do
  ast=CoreExport.rebuild_terms(row["parse"]["ast"])
  forms=case row["helper"] do
   "t" -> for t<-if(is_list(ast),do: ast,else: [ast]),do: {:attribute,0,:type,{:t,t,[]}}
   "sig" -> [{:attribute,0,:spec,{{:a,:b,0},ast}}]
  end
  for {form,i}<-Enum.with_index(forms) do
   doc=:syntax_native_pp.lform(form,:syntax_native_pp.options(opts))
   expected=:syntax_native_pp.frmt(doc,:syntax_native_pp.state(opts))|>:lists.flatten()|>List.to_string()
   IO.puts(:json.encode(%{id: "#{row["id"]}-#{i}",source: row["source"],helper: row["helper"],doc: CoreExport.export_terms(doc),expected: expected}))
  end
 end
end
