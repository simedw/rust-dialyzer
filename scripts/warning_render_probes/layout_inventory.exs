Code.require_file("../core_export.exs",__DIR__)
{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer),[:abstract_code])
replacements=Map.new([:t,:sig,:field_diffs],&{&1,String.to_atom("original_"<>Atom.to_string(&1))})
forms=Enum.map(forms,fn
 {:attribute,a,:module,_}->{:attribute,a,:module,:layout_native}
 {:function,a,name,arity,clauses}=f -> case replacements do %{^name=>renamed}->{:function,a,renamed,arity,clauses};_->f end
 f->f
end)
wrappers=Enum.map(replacements,fn {name,original}->
 src = ~c"#{name}(Src,I) -> Result=#{original}(Src,I), case I of true->put({layout,#{name},Src},Result);_->ok end, Result."
 {:ok,tokens,_}=:erl_scan.string(src);{:ok,form}=:erl_parse.parse_form(tokens);form
end)
forms=Enum.reject(forms,&match?({:eof,_},&1))++wrappers
{:ok,:layout_native,binary,_}=:compile.forms(forms,[:binary,:export_all,:return_errors,:return_warnings])
{:module,:layout_native}=:code.load_binary(:layout_native,~c"layout_native",binary)
for filename<-System.argv() do
 File.stream!(filename)|>Enum.each(fn line -> row=:json.decode(line);if row["options"]["indent"] do
  warning=CoreExport.rebuild_terms(row["warning"])
  :layout_native.format_warning(warning,[indent_opt: true])
 end end)
end
values=Process.get()|>Enum.filter(fn {key,_}->match?({:layout,_,_},key)end)|>Enum.sort_by(fn {{_,helper,src},_}->{helper,src}end)
for {{{_,helper,src},rendered},index}<-Enum.with_index(values) do
 parse=case helper do
  :t -> try do
    ast=if match?([?(|_],src),do: :layout_native.parse_types_and_literals(Enum.drop(src,1)|>Enum.drop(-1)),else: :layout_native.parse_type_or_literal(src)
    %{status: "parsed",ast: CoreExport.export_terms(ast)}
   catch kind,reason->%{status: "fallback",class: Atom.to_string(kind),reason: CoreExport.export_terms(reason)}end
  :sig -> try do
    text = ~c"-spec a:b"++src++~c"."
    {:ok,tokens,_}=:erl_scan.string(text);{:ok,{:attribute,_,:spec,{_,types}}}=:erl_parse.parse_form(tokens)
    %{status: "parsed",ast: CoreExport.export_terms(types)}
   catch kind,reason->%{status: "fallback",class: Atom.to_string(kind),reason: CoreExport.export_terms(reason)}end
  _ ->%{status: "fields"}
 end
 IO.puts(:json.encode(%{id: index,helper: Atom.to_string(helper),source: List.to_string(src),expected: List.to_string(:lists.flatten(rendered)),parse: parse}))
end
