Code.require_file("type_codec.exs",__DIR__)
[manifest,out] = System.argv()
cases = manifest |> File.read!() |> :json.decode()
results = Enum.map(cases, fn c ->
  rust = c["rust_json"] |> File.read!() |> :json.decode()
  beams = c["beams_dir"] |> Path.join("*.beam") |> Path.wildcard() |> Enum.map(&String.to_charlist/1)
  plt_path=Path.join(Path.dirname(out),c["name"]<>".plt") |> String.to_charlist()
  :dialyzer.run(analysis_type: :plt_build,files: beams,output_plt: plt_path,get_warnings: false,warnings: [:no_unknown],check_plt: false)
  plt=:dialyzer_cplt.from_file(plt_path)
  checks=Enum.flat_map(rust["signatures"],fn s ->
    case Regex.run(~r/module: "([^"]+)", name: "([^"]+)", arity: (\d+)/,s["identity"]) do
    nil -> []
    [_,module,function,arity] ->
    case :dialyzer_plt.lookup(plt,{String.to_atom(module),String.to_atom(function),String.to_integer(arity)}) do
      {:value,{native_result,native_args}} ->
        actual_result=TypeCodec.type(s["result"]);actual_args=Enum.map(s["arguments"],&TypeCodec.type/1)
        [%{module: module,function: function,arity: arity,exported: s["exported"],args_match: native_args==actual_args,result_match: native_result==actual_result,native_args: Enum.map(native_args,&(:erl_types.t_to_string(&1)|>List.to_string())),native_result: :erl_types.t_to_string(native_result)|>List.to_string(),rust_args: s["arguments"],rust_result: s["result"]}]
      :none -> [%{module: module,function: function,arity: arity,exported: s["exported"],native_status: "not_persisted_private",rust_args: s["arguments"],rust_result: s["result"]}]
    end
    end
  end)
  warnings=:dialyzer.run(files: beams,init_plt: plt_path,check_plt: false,warnings: [:no_unknown],error_location: :column,indent_opt: false)
  show=fn t->t|>TypeCodec.type()|>:erl_types.t_to_string()|>List.to_string() end
  native_warnings=Enum.map(warnings,fn {tag,{file,{line,col}},detail}->
    actual=Enum.find(rust["warnings"],&(&1["line"]==line and &1["column"]==col))
    details_match=if actual do
      a=actual["details"]
      case detail do
        {:call,[m,f,arg_text,_positions,_reason,domain_text,_return,_contract]} ->
          args="("<>Enum.map_join(a["actual"],", ",show)<>")"
          domain="("<>Enum.map_join(a["expected"],", ",show)<>")"
          a["target"]["module"]==Atom.to_string(m) and a["target"]["name"]==Atom.to_string(f) and args==List.to_string(arg_text) and domain==List.to_string(domain_text)
        {:guard_fail,[left,op,right]} ->
          a["operation"]["name"]==List.to_string(op) and length(a["arguments"])==2 and String.ends_with?(List.to_string(left),show.(Enum.at(a["arguments"],0))) and String.ends_with?(List.to_string(right),show.(Enum.at(a["arguments"],1)))
        {kind,[_pattern,type_text]} when kind in [:pattern_match,:pattern_match_cov] ->
          a["category"]==Atom.to_string(kind) and show.(a["actual"])==List.to_string(type_text)
        {:app_call,[_m,_f,_args,_culprit,expected,actual_type]} -> a["category"]=="app_call" and show.(a["expected"])==List.to_string(expected) and show.(a["actual"])==List.to_string(actual_type)
        {:no_return,[_class,_name,_arity]} -> a["category"]=="no_return" and a["result"]["kind"] in ["none","unit"]
        _ -> false
      end
    else false end
    %{tag: tag,line: line,column: col,file: List.to_string(file),detail: inspect(detail,limit: :infinity,charlists: :as_lists),message: :dialyzer.format_warning({tag,{file,{line,col}},detail})|>List.to_string(),typed_details_match: details_match}
  end)
  %{id: c["name"],signatures: checks,native_warnings: native_warnings,rust_warnings: rust["warnings"]}
end)
File.write!(out,:json.encode(results))
