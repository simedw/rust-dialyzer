Code.require_file("native_types.exs", __DIR__)
Code.require_file("core_export.exs", __DIR__)
Code.ensure_loaded!(:erl_types)
defmodule WarningOracle do
  def forms(module) do
    {:ok, {_, [abstract_code: {:raw_abstract_v1, forms}]}} = :beam_lib.chunks(:code.which(module), [:abstract_code])
    forms
  end
  def field_names(forms, name) do
    {:attribute, _, :record, {^name, fields}} = Enum.find(forms, fn f -> match?({:attribute, _, :record, {^name, _}}, f) end)
    Enum.map(fields, fn
      {:typed_record_field, field, _} -> field
      field -> field
    end) |> Enum.map(fn field -> elem(elem(field, 2), 2) end)
  end
  def index(fields, name), do: Enum.find_index(fields, &(&1 == name)) + 1
  def load do
    forms = forms(:dialyzer_dataflow)
    helper = ~c"make_state(CG, Plt, Fun, Ret, Handled, Reachable, Mode) -> L=cerl_trees:get_label(Fun), E=case Handled of true->{[],Ret};false->{not_handled,{[],Ret}} end, #state{callgraph=CG,plt=Plt,tree_map=dict:from_list([{L,Fun}]),fun_tab=dict:from_list([{L,E}]),fun_homes=dict:from_list([{L,{m,f,0}}]),reachable_funs=sets:from_list(case Reachable of true->[L];false->[] end),warning_mode=Mode,module=m}."
    {:ok,tokens,_}=:erl_scan.string(helper)
    {:ok,helper_form}=:erl_parse.parse_form(tokens)
    forms=Enum.map(forms,fn {:attribute,a,:module,_}->{:attribute,a,:module,:warning_native};f->f end)
    {:ok,ctokens,_}=:erl_scan.string(~c"comp_format_args(_, T1, Op, T2, _) -> [T1, Op, T2].")
    {:ok,cform}=:erl_parse.parse_form(ctokens)
    forms=Enum.reject(forms,fn f -> match?({:eof,_},f) or match?({:function,_,:comp_format_args,5,_},f) end)++[helper_form,cform]
    {:ok,:warning_native,binary,_}=:compile.forms(forms,[:binary,:export_all,:return_errors,:return_warnings])
    {:module,:warning_native}=:code.load_binary(:warning_native,~c"warning_native",binary)
    af=Enum.map(forms(:dialyzer_analysis_callgraph),fn {:attribute,a,:module,_}->{:attribute,a,:module,:warning_native_analysis};f->f end)
    {:ok,:warning_native_analysis,ab,_}=:compile.forms(af,[:binary,:export_all,:return_errors,:return_warnings])
    {:module,:warning_native_analysis}=:code.load_binary(:warning_native_analysis,~c"warning_native_analysis",ab)
    %{state: field_names(forms,:state),graph: field_names(forms(:dialyzer_callgraph),:callgraph),ast_hash: Base.encode16(:crypto.hash(:sha256,:erlang.term_to_binary(forms)),case: :lower),analysis_hash: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(:code.which(:dialyzer_analysis_callgraph)))),case: :lower),hash: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(:code.which(:dialyzer_dataflow)))),case: :lower)}
  end
  def type(value) when value in [nil,:null],do: nil
  def type(value),do: NativeTypes.decode(value)
  def contract(return) do
    signature=:erl_types.t_fun([] , return)
    {:contract,[{signature,[]}],[],[]}
  end
  def body("only_explicit"),do: :cerl.c_call(:cerl.c_atom(:erlang),:cerl.c_atom(:error),[:cerl.c_atom(:ok)])
  def body("no_match"),do: :cerl.c_primop(:cerl.c_atom(:match_fail),[])
  def body("both") do
    :cerl.c_case(:cerl.c_atom(:ok),[:cerl.c_clause([:cerl.c_atom(:ok)],body("only_normal")),:cerl.c_clause([:cerl.c_atom(:bad)],body("only_explicit"))])
  end
  def body(_),do: :cerl.c_atom(:ok)
  def run(row, profile) do
    cg=:dialyzer_callgraph.new();plt=:dialyzer_plt.new()
    graph=elem(cg,index(profile.graph,:digraph));names=elem(cg,index(profile.graph,:name_map));esc=elem(cg,index(profile.graph,:esc))
    :digraph.add_vertex(graph,1);:digraph.add_vertex(graph,2);:digraph.add_vertex(graph,3)
    if row["named"],do: :ets.insert(names,{1,{:m,:f,0}})
    if row["parent_named"],do: :ets.insert(names,{2,{:m,:p,0}})
    if row["escaping"],do: :ets.insert(esc,{1})
    for parent<-Enum.take([2,3],row["incoming_count"]||0),do: :digraph.add_edge(graph,parent,1)
    if row["own_contract"] not in [nil,:null],do: :dialyzer_plt.insert_contract_list(plt,[{{:m,:f,0},contract(type(row["own_contract"]))}])
    if row["parent_contract"] not in [nil,:null],do: :dialyzer_plt.insert_contract_list(plt,[{{:m,:p,0},contract(type(row["parent_contract"]))}])
    ann=Enum.map(row["annotations"]||[1],fn "compiler_generated"->:compiler_generated;[line,column]->{line,column};value->value end)
    fun=:cerl.ann_c_fun([{:label,1}|ann],[],body(row["return_kind"]))
    state=:warning_native.make_state(cg,plt,fun,type(row["result"]),row["handled"]==true,row["reachable"]==true,row["warning_mode"]==true)
    warnings=case row["operation"] do
      "policy" ->
        cs=:dialyzer_codeserver.new()
        props=(if row["module_tag"],do: [:warn_failing_call],else: [])++(if row["module_suppressed"],do: [:nowarn_function],else: [])
        meta=if row["module_metadata"],do: [{:m,props}],else: []
        fp=Enum.flat_map([{:warn_failing_call,row["function_tag"]},{:nowarn_function,row["function_suppression"]}],fn {property,value}->case value do "enabled"->[{property,:func}];"disabled"->[{property,:mod}];_->[] end end)
        cs=:dialyzer_codeserver.insert_fun_meta_info(meta++[{{:m,:f,0},fp}],cs)
        owner=if row["named"],do: {:m,:f,0},else: :m
        filtered=:warning_native_analysis.filter_warnings([{:warn_failing_call,{[],1,owner},:probe}],cs)
        :dialyzer_codeserver.delete(cs)
        filtered
      "collection" ->
        old=Enum.map(row["old"],&String.to_atom/1);new=Enum.map(row["new"],&String.to_atom/1)
        initial=:warning_native.state__renew_warnings(old,state)
        extended=:warning_native.state__add_warnings(new,initial)
        {added,reset}=:warning_native.state__remove_added_warnings(initial,extended)
        {:effects,elem(extended,index(profile.state,:warnings)),added,elem(reset,index(profile.state,:warnings))}
      "compare" -> new=:warning_native.add_bif_warnings({String.to_atom(row["module"]||"erlang"),String.to_atom(row["operator"]),2},[type(row["result"]),type(row["right"])],fun,state);elem(new,index(profile.state,:warnings))
      "add" -> new=:warning_native.state__add_warning(state,:warn_failing_call,fun,:probe,row["forced"]);elem(new,index(profile.state,:warnings))
      _ -> :warning_native.state__get_warnings(state)
    end
    :dialyzer_plt.delete(plt)
    :dialyzer_callgraph.delete(cg)
    for name<-[:esc,:letrec_map,:name_map,:rev_name_map,:rec_var_map,:self_rec,:calls],do: :ets.delete(elem(cg,index(profile.graph,name)))
    %{id: row["id"], request: row, input_types: CoreExport.export_terms({type(row["result"]), type(row["own_contract"]), type(row["parent_contract"]),type(row["right"])}), profile: %{beam_sha256: profile.hash, analysis_beam_sha256: profile.analysis_hash, instrumented_ast_sha256: profile.ast_hash}, warnings: CoreExport.export_terms(warnings)}
  end
end
profile=WarningOracle.load()
IO.stream(:stdio,:line)|>Enum.each(fn line -> row=:json.decode(line);result=WarningOracle.run(row,profile);IO.puts(:json.encode(result)) end)
