Code.require_file("type_codec.exs", __DIR__)
defmodule ScalarOracle do
  defdelegate type(value), to: TypeCodec
  defdelegate integer(value), to: TypeCodec
  def native_value(v) do
    case v["term"] do
      "integer" -> integer(v["value"])
      "atom" -> String.to_atom(v["value"])
      "nil" -> []
      "tuple" -> v["values"] |> Enum.map(&native_value/1) |> List.to_tuple()
      "list" -> Enum.map(v["values"],&native_value/1)
    end
  end
  def getter_value(v) do
    case v["kind"] do
      "type" -> type(v["value"])
      "types" -> if v["values"]==:null,do: :unknown,else: Enum.map(v["values"],&type/1)
      "atoms" -> if v["values"]==:null,do: :unknown,else: Enum.map(v["values"],&String.to_atom/1)
      "integers" -> if v["values"]==:null,do: :unknown,else: Enum.map(v["values"],&integer/1)
      "arity" -> if v["value"]==:null,do: :unknown,else: v["value"]
      "arities" -> if v["values"]==:null,do: :unknown,else: v["values"]
      "integer" -> integer(v["value"])
      "endpoint" -> integer(v["value"])
      "atom" -> String.to_atom(v["value"])
      "variable" -> if v["value"]["atom"],do: String.to_atom(v["value"]["atom"]),else: integer(v["value"]["integer"])
      "entries" -> Enum.map(v["values"],fn e->{type(e["key"]),String.to_atom(e["presence"]),type(e["value"])}end)
    end
  end
  def runtime_term(v) do
    case v["term"] do
      "atom"->String.to_atom(v["value"])
      "integer"->integer(v["value"])
      "float"->v["value"]*1.0
      "bitstring"-><<integer(v["value"]||"0")::size(v["bits"])>>
      "function"->identity=v["identity"];case v["arity"] do 0->fn ->identity end;1->fn _->identity end;2->fn _,_->identity end;3->fn _,_,_->identity end end
      "pid"->self()
      "port"->hd(:erlang.ports())
      "reference"->cache=Process.get(:runtime_refs,%{});ref=Map.get(cache,v["identity"],make_ref());Process.put(:runtime_refs,Map.put(cache,v["identity"],ref));ref
      "nil"->[]
      "cons"->[runtime_term(v["head"])|runtime_term(v["tail"])]
      "tuple"->List.to_tuple(Enum.map(v["elements"],&runtime_term/1))
      "map"->Map.new(v["pairs"],fn [k,v]->{runtime_term(k),runtime_term(v)}end)
    end
  end
  def records(command) do
    Enum.reduce(command["records"]||[],%{},fn r,table ->
      key={:record,String.to_atom(r["name"])};fields=Enum.map(r["fields"],fn f->{String.to_atom(f["name"]),{:type,0,:any,[]},type(f["type"])}end)
      previous=case Map.get(table,key) do nil->[];{_,p}->p end
      Map.put(table,key,{0,:orddict.store(length(fields),fields,previous)})
    end)
  end
  def arena_term(arena,id) do
    t=Enum.at(arena["terms"],id)
    case t["kind"] do
      "atom"->String.to_atom(t["value"])
      "integer"->integer(t["value"])
      "float"-><<v::float-64>>=Base.decode16!(t["value"]);v
      "binary"->Base.decode64!(t["value"])
      "bitstring"->b=Base.decode64!(t["bytes"]);bits=t["bits"];<<v::bitstring-size(bits),_::bitstring>>=b;v
      "tuple"->List.to_tuple(Enum.map(t["items"],&arena_term(arena,&1)))
      "nil"->[]
      "list"->Enum.reduce(Enum.reverse(t["items"]),arena_term(arena,t["tail"]),fn id,tail->[arena_term(arena,id)|tail]end)
      "map"->Map.new(t["entries"],fn [k,v]->{arena_term(arena,k),arena_term(arena,v)}end)
    end
  end
  def expected(command) do
    a = type(command["a"])
    case command["op"] do
      "substitute_all_variables" -> :erl_types.subst_all_vars_to_any(a)
      "meet_lists" -> :erl_types.t_inf_lists(Enum.map(command["left"],&type/1),Enum.map(command["right"],&type/1))
      "map_pairs" -> :erl_types.t_map(Enum.map(command["pairs"],fn [k,v]->{type(k),type(v)} end))
      "unification_limit" -> :erl_types.t_limit(type(command["binding"]),command["depth"])
      "form_to_string" -> :erl_types.t_form_to_string(arena_term(command["arena"],command["arena"]["root"])) |> :lists.flatten() |> List.to_string()
      "atom_to_string" -> :erl_types.atom_to_string(String.to_atom(command["name"])) |> List.to_string()
      "type_to_string" -> :erl_types.t_to_string(a,records(command)) |> List.to_string()
      "record_field_diffs" -> {positions,text}=:erl_types.record_field_diffs_to_string(a,records(command));%{"positions"=>positions,"text"=>List.to_string(text)}
      "atoms_constructor" -> :erl_types.t_atoms(Enum.map(command["values"],&String.to_atom/1))
      "function_arity" -> :erl_types.t_fun(command["arity"],type(command["range"]))
      "number_literal" -> :erl_types.t_number(integer(command["value"]))
      "endpoint_min" -> :erl_types.min(integer(command["left"]),integer(command["right"]))
      "endpoint_max" -> :erl_types.max(integer(command["left"]),integer(command["right"]))
      "map_pairwise_merge" -> f=fn k,p,x,q,y -> case command["mode"] do "skip"->false;"left"->{k,p,x};"right"->{k,q,y};"join"->{k,if(p==:mandatory or q==:mandatory,do: :mandatory,else: :optional),:erl_types.t_sup(x,y)} end end;:erl_types.t_map_pairwise_merge(f,a,type(command["b"]))
      "from_runtime_term" -> :erl_types.t_from_term(runtime_term(command["term"]))
      "widen_to_number" -> :erl_types.t_widen_to_number(a)
      "native_order" -> b=type(command["b"]);cond do a<b->"less";a>b->"greater";true->"equal" end
      "getter" -> name=case command["name"] do
        "atom_values"->"t_atom_vals"; "number_values"->"t_number_vals"; "number_min"->"number_min"; "number_max"->"number_max"; "bitstring_unit"->"t_bitstr_unit"; "bitstring_base"->"t_bitstr_base"; "function_args"->"t_fun_args"; "function_arity"->"t_fun_arity"; "function_range"->"t_fun_range"; "cons_head"->"t_cons_hd"; "cons_tail"->"t_cons_tl"; "map_default_key"->"t_map_def_key"; "map_default_value"->"t_map_def_val"; "variable_name"->"t_var_name"; "to_type_list"->"t_to_tlist"; other->"t_"<>other
      end; apply(:erl_types,String.to_atom(name),[a])
      "lift_list" -> :erl_types.lift_list_to_pos_empty(a)
      "is_native_type" -> :erl_types.is_erl_type(a)
      "supremum" -> :erl_types.t_sup(Enum.map(command["types"],&type/1))
      "infimum" -> :erl_types.t_inf(Enum.map(command["types"],&type/1))
      "subtract_types" -> :erl_types.t_subtract_list(a,Enum.map(command["types"],&type/1))
      "bitstr_concat_types" -> :erl_types.t_bitstr_concat(Enum.map(command["types"],&type/1))
      "any_none" -> :erl_types.any_none(Enum.map(command["types"],&type/1))
      "any_none_or_unit" -> :erl_types.any_none_or_unit(Enum.map(command["types"],&type/1))
      "builtin" -> name=case command["name"] do
        "bitstring" -> "bitstr"; "function" -> "fun"; "positive_integer" -> "pos_integer"; "non_negative_integer" -> "non_neg_integer"; "negative_integer" -> "neg_integer"; "positive_fixnum" -> "pos_fixnum"; "non_negative_fixnum" -> "non_neg_fixnum"; "bitstring_list" -> "bitstrlist"; other -> other
      end; apply(:erl_types,String.to_atom("t_"<>name),[])
      "predicate" -> name=case command["name"] do
        "bitstring" -> "bitstr"; "function" -> "fun"; "non_negative_integer" -> "non_neg_integer"; "variable" -> "var"; other -> other
      end; apply(:erl_types,String.to_atom("t_is_"<>name),[a])
      "atom_literal" -> :erl_types.t_is_any_atom(String.to_atom(command["name"]),a)
      "same_opaque" -> :erl_types.t_is_same_opaque(a,type(command["b"]))
      "unify" -> :erl_types.t_unify_table_only(a,type(command["b"]))
      "substitute" -> :erl_types.t_subst(a,Map.new(command["bindings"],fn e->{if(e["id"]["atom"],do: String.to_atom(e["id"]["atom"]),else: integer(e["id"]["integer"])),type(e["type"])} end))
      "has_var" -> :erl_types.t_has_var(a)
      "collect_var_names" -> Enum.map(:erl_types.t_collect_var_names(a), fn id -> if is_atom(id), do: %{"atom"=>Atom.to_string(id)}, else: %{"integer"=>Integer.to_string(id)} end)
      "elements" -> :erl_types.t_elements(a)
      "structural" -> :erl_types.t_structural(a)
      "is_opaque" -> :erl_types.t_is_opaque(a,String.to_atom(command["module"]))
      "opacity_conflict" -> :erl_types.t_opacity_conflict(a,type(command["b"]),String.to_atom(command["module"])) |> Atom.to_string()
      "canonical" -> a
      "limit" -> :erl_types.t_limit(a, command["depth"])
      "map_get" -> :erl_types.t_map_get(type(command["b"]), a)
      "map_is_key" -> :erl_types.t_map_is_key(type(command["b"]), a)
      "map_remove" -> :erl_types.t_map_remove(type(command["b"]), a)
      "map_put" -> :erl_types.t_map_put({type(command["b"]),type(command["value"])},a)
      "map_update" -> :erl_types.t_map_update({type(command["b"]),type(command["value"])},a)
      "bitstr_concat" -> :erl_types.t_bitstr_concat(a,type(command["b"]))
      "bitstr_match" -> :erl_types.t_bitstr_match(a,type(command["b"]))
      "join" -> :erl_types.t_sup(a, type(command["b"]))
      "meet" -> :erl_types.t_inf(a, type(command["b"]))
      "subtract" -> :erl_types.t_subtract(a, type(command["b"]))
      "subtype" -> :erl_types.t_is_subtype(a, type(command["b"]))
    end
  end
  def run(commands, results) do
    inputs = read(commands)
    outputs = read(results)
    if length(inputs) != length(outputs), do: raise("different result count")
    Process.put(:native_values,0)
    Process.put(:native_errors,%{})
    Process.put(:native_timeouts,[])
    failures = Enum.zip(inputs,outputs) |> Enum.with_index() |> Enum.flat_map(fn {{command,output}, index} ->
      try do
        native_started = System.monotonic_time(:millisecond)
        native_task = Task.async(fn -> try do {:ok, expected(command)} catch kind, reason -> {:error, kind, reason} end end)
        native = case Task.yield(native_task,1000) || Task.shutdown(native_task,:brutal_kill) do
          {:ok,result}->result
          nil->:timeout
        end
        case native do
          :timeout ->
            Process.put(:native_timeouts,[%{index: index,timeout_ms: 1000,elapsed_ms: System.monotonic_time(:millisecond)-native_started,command: command,rust: output}|Process.get(:native_timeouts)])
            if String.starts_with?(output["error"] || "", "native_nonterminating"), do: [], else: [%{index: index,command: command,rust: output,expected_error: "native_timeout"}]
          {:ok, expected} ->
            Process.put(:native_values,Process.get(:native_values)+1)
            actual = cond do
              command["op"] == "record_field_diffs" -> output["ok"]
              command["op"] in ["getter","endpoint_min","endpoint_max","map_pairwise_merge"] -> getter_value(output["ok"])
              command["op"] == "unify" -> Map.new(output["ok"],fn e->{if(e["id"]["atom"],do: String.to_atom(e["id"]["atom"]),else: integer(e["id"]["integer"])),type(e["type"])} end)
              command["op"] == "collect_var_names" -> output["ok"]
              is_list(output["ok"]) -> Enum.map(output["ok"],&type/1)
              is_boolean(output["ok"]) or is_binary(output["ok"]) -> output["ok"]
              true -> type(output["ok"])
            end
            same = cond do
              is_list(expected) -> is_list(actual) and length(actual)==length(expected) and Enum.all?(Enum.zip(actual,expected),fn {x,y}->:erl_types.t_is_equal(x,y) end)
              is_boolean(expected) or is_binary(expected) or is_map(expected) -> actual==expected
              true -> :erl_types.t_is_equal(actual,expected)
            end
            if same, do: [], else: [%{index: index, command: command, rust: output, expected_term: inspect(expected, limit: :infinity), actual_term: inspect(actual, limit: :infinity)}]
          {:error, :throw, {:mismatch,left,right}} ->
            prefix="OTP throw mismatch: "
            if String.starts_with?(output["error"]||"",prefix) do
              [actual_left,actual_right]=:json.decode(String.replace_prefix(output["error"],prefix,""))
              if type(actual_left)==left and type(actual_right)==right,do: record_native_error("throw:mismatch"),else: [%{index: index,command: command,rust: output,expected_error: inspect({:throw,{:mismatch,left,right}},limit: :infinity)}]
            else
              [%{index: index,command: command,rust: output,expected_error: inspect({:throw,{:mismatch,left,right}},limit: :infinity)}]
            end
          {:error, :error, :function_clause} ->
            if String.starts_with?(output["error"] || "", "OTP function_clause"), do: record_native_error("function_clause"), else: [%{index: index, command: command, rust: output, expected_error: "error: function_clause"}]
          {:error, :error, :badarg} ->
            if String.starts_with?(output["error"] || "", "OTP badarg"), do: record_native_error("badarg"), else: [%{index: index, command: command, rust: output, expected_error: "error: badarg"}]
          {:error, :error, {:bad_generator,payload}} ->
            prefix="OTP bad_generator: "
            if String.starts_with?(output["error"]||"",prefix) and type(:json.decode(String.replace_prefix(output["error"],prefix,"")))==payload,do: record_native_error("bad_generator"),else: [%{index: index,command: command,rust: output,expected_error: inspect({:bad_generator,payload},limit: :infinity)}]
          {:error, :error, {:bad_generators,{left,right}}} ->
            prefix="OTP bad_generators: "
            if String.starts_with?(output["error"]||"",prefix) do
              [actual_left,actual_right]=:json.decode(String.replace_prefix(output["error"],prefix,""))
              if Enum.map(actual_left,&type/1)==left and Enum.map(actual_right,&type/1)==right,do: record_native_error("bad_generators"),else: [%{index: index,command: command,rust: output,expected_error: inspect({:bad_generators,{left,right}},limit: :infinity)}]
            else
              [%{index: index,command: command,rust: output,expected_error: inspect({:bad_generators,{left,right}},limit: :infinity)}]
            end
          {:error, :error, {:badmatch,payload}} ->
            native_prefix="OTP badmatch native: "
            prefix="OTP badmatch: "
            actual=cond do
              String.starts_with?(output["error"],native_prefix)->native_value(:json.decode(String.replace_prefix(output["error"],native_prefix,"")))
              String.starts_with?(output["error"],prefix)->raw=:json.decode(String.replace_prefix(output["error"],prefix,""));if is_boolean(raw),do: raw,else: type(raw)
              true->:mismatching_error
            end
            if actual==payload,do: record_native_error("badmatch"),else: [%{index: index,command: command,rust: output,expected_error: inspect({:badmatch,payload},limit: :infinity)}]
          {:error, :error, {:case_clause,payload}} ->
            prefix = "OTP case_clause: "
            if String.starts_with?(output["error"] || "",prefix) and :erl_types.t_is_equal(type(:json.decode(String.replace_prefix(output["error"],prefix,""))),payload), do: record_native_error("case_clause"), else: [%{index: index,command: command,rust: output,expected_error: inspect({:case_clause,payload},limit: :infinity)}]
          {:error, kind, reason} ->
            [%{index: index, command: command, rust: output, expected_error: inspect({kind,reason})}]
        end
      rescue
        e -> [%{index: index, command: command, rust: output, error: Exception.message(e)}]
      catch
        kind, reason -> [%{index: index, command: command, rust: output, error: inspect({kind,reason})}]
      end
    end)
    report = %{commands: length(inputs), passed: length(inputs)-length(failures), native_values: Process.get(:native_values), native_timeouts: Enum.reverse(Process.get(:native_timeouts)), runtime: %{otp: :erlang.system_info(:otp_release) |> List.to_string(), erts: :erlang.system_info(:version) |> List.to_string()}, native_error_matches: Process.get(:native_errors), unsupported: Enum.count(outputs, &String.starts_with?(&1["error"] || "", "unsupported")), failures: failures}
    File.write!(results <> ".comparison.json", :json.encode(report))
    IO.puts("#{report.passed}/#{report.commands} OTP comparisons passed")
    if failures != [], do: System.halt(1)
  end
  def record_native_error(class) do
    Process.put(:native_errors,Map.update(Process.get(:native_errors),class,1,&(&1+1)))
    []
  end
  def read(path), do: File.read!(path) |> String.split("\n", trim: true) |> Enum.map(&:json.decode/1)
end
[commands,results] = System.argv()
ScalarOracle.run(commands,results)
