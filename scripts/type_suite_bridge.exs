Code.require_file("bridge_native_types.exs",__DIR__)
defmodule RustTypeBridge do
  @builtins ~w(any none unit atom binary bitstr boolean byte char fun identifier pid port reference number float integer pos_integer non_neg_integer neg_integer fixnum pos_fixnum non_neg_fixnum cons nil list nonempty_list string nonempty_string maybe_improper_list map tuple arity mfa module node iolist iodata bitstrlist timeout nonempty_binary nonempty_bitstring)
  def type(t),do: RustBridgeTypeWire.encode(t)
  def names(name) do
    case name do
      "bitstr"->"bitstring";"fun"->"function";"var"->"variable";"pos_integer"->"positive_integer";"non_neg_integer"->"non_negative_integer";"neg_integer"->"negative_integer";"pos_fixnum"->"positive_fixnum";"non_neg_fixnum"->"non_negative_fixnum";"bitstrlist"->"bitstring_list";other->other
    end
  end
  def records(table) do
    for {{:record,name},{_,alternatives}}<-table,{_,fields}<-alternatives do
      %{name: to_string(name),fields: Enum.map(fields,fn {n,_,t}->%{name: to_string(n),type: type(t)}end)}
    end
  end
  def runtime(t) when is_atom(t),do: %{term: "atom",value: to_string(t)}
  def runtime(t) when is_integer(t),do: %{term: "integer",value: to_string(t)}
  def runtime(t) when is_float(t),do: %{term: "float",value: t}
  def runtime([]),do: %{term: "nil"}
  def runtime([h|t]),do: %{term: "cons",head: runtime(h),tail: runtime(t)}
  def runtime(t) when is_tuple(t),do: %{term: "tuple",elements: Enum.map(Tuple.to_list(t),&runtime/1)}
  def runtime(t) when is_map(t),do: %{term: "map",pairs: Enum.map(t,fn{k,v}->[runtime(k),runtime(v)]end)}
  def request(name,args) do
    base=%{a: %{kind: "none"},native_output: true}
    op=case {name,args} do
      {:t_to_string,[t]}->%{op: "type_to_string",a: type(t)}
      {:t_to_string,[t,table]}->%{op: "type_to_string",a: type(t),records: records(table)}
      {:record_field_diffs_to_string,[t,table]}->%{op: "record_field_diffs",a: type(t),records: records(table)}
      {:atom_to_string,[a]}->%{op: "atom_to_string",name: to_string(a)}
      {:t_from_term,[t]}->%{op: "from_runtime_term",term: runtime(t)}
      {:t_atom,[a]}->%{op: "atoms_constructor",values: [to_string(a)]}
      {:t_atoms,[a]}->%{op: "atoms_constructor",values: Enum.map(a,&to_string/1)}
      {name,[n]} when name in [:t_integer,:t_number]->%{op: "number_literal",value: to_string(n)}
      {:t_integers,[ns]}->%{op: "canonical",a: %{kind: "integers",values: Enum.map(ns,&to_string/1)}}
      {:t_from_range,[a,b]}->%{op: "canonical",a: %{kind: "range",from: to_string(a),to: to_string(b)}}
      {:t_tuple,[n]} when is_integer(n)->%{op: "canonical",a: %{kind: "tuple_arity",arity: n}}
      {:t_tuple,[ts]}->%{op: "canonical",a: %{kind: "tuple",elements: Enum.map(ts,&type/1)}}
      {:t_fun,[range]}->%{op: "canonical",a: %{kind: "function",domain: :null,range: type(range)}}
      {:t_fun,[arity,range]} when is_integer(arity)->%{op: "function_arity",arity: arity,range: type(range)}
      {:t_fun,[domain,range]}->%{op: "canonical",a: %{kind: "function",domain: Enum.map(domain,&type/1),range: type(range)}}
      {:t_product,[ts]}->%{op: "canonical",a: %{kind: "product",values: Enum.map(ts,&type/1)}}
      {:t_bitstr,[u,b]}->%{op: "canonical",a: %{kind: "bitstring",unit: to_string(u),base: to_string(b)}}
      {name,[t]} when name in [:t_list,:t_nonempty_list]->%{op: "canonical",a: %{kind: "proper_list",elements: type(t),nonempty: name==:t_nonempty_list}}
      {:t_cons,[h,t]}->%{op: "canonical",a: %{kind: "cons",head: type(h),tail: type(t)}}
      {:t_limit,[t,k]}->%{op: "limit",a: type(t),depth: k}
      {:t_sup,[ts]}->%{op: "supremum",types: Enum.map(ts,&type/1)}
      {:t_inf,[ts]}->%{op: "infimum",types: Enum.map(ts,&type/1)}
      {name,[a,b]} when name in [:t_sup,:t_inf,:t_subtract,:t_is_subtype]->%{op: %{t_sup: "join",t_inf: "meet",t_subtract: "subtract",t_is_subtype: "subtype"}[name],a: type(a),b: type(b)}
      {:t_map,[pairs,dk,dv]}->%{op: "canonical",a: %{kind: "map",entries: Enum.map(pairs,fn{k,p,v}->%{key: type(k),presence: to_string(p),value: type(v)}end),default_key: type(dk),default_value: type(dv)}}
      {name,[]}->s=to_string(name);if String.starts_with?(s,"t_") && String.replace_prefix(s,"t_","") in @builtins,do: %{op: "builtin",name: names(String.replace_prefix(s,"t_",""))},else: throw({:unsupported_bridge,{name,0}})
      {name,[t]}->s=to_string(name);cond do
        String.starts_with?(s,"t_is_")->%{op: "predicate",name: names(String.replace_prefix(s,"t_is_","")),a: type(t)}
        name==:t_cons_hd->%{op: "getter",name: "cons_head",a: type(t)}
        name==:t_cons_tl->%{op: "getter",name: "cons_tail",a: type(t)}
        true->throw({:unsupported_bridge,{name,1}})
      end
      _->throw({:unsupported_bridge,{name,length(args)}})
    end
    Map.merge(base,op)
  end
  def decode(%{"term"=>"atom","value"=>v}),do: String.to_atom(v)
  def decode(%{"term"=>"integer","value"=>v}),do: String.to_integer(v)
  def decode(%{"term"=>"tuple","values"=>v}),do: List.to_tuple(Enum.map(v,&decode/1))
  def decode(%{"term"=>"nil"}),do: []
  def decode(%{"term"=>"list","values"=>v}),do: Enum.map(v,&decode/1)
  def call(name,args) do
    key="#{name}/#{length(args)}";Process.put(:bridge_counts,Map.update(Process.get(:bridge_counts,%{}),key,1,&(&1+1)))
    command=request(name,args);port=Process.get(:rust_port)
    true=Port.command(port,IO.iodata_to_binary(:json.encode(command))<>"\n")
    reply=receive do {^port,{:data,{:eol,line}}}->:json.decode(line) after 10000->raise "Rust bridge timeout" end
    case reply do
      %{"ok"=>%{"native"=>v}}->decode(v)
      %{"ok"=>v} when is_boolean(v)->v
      %{"ok"=>v} when is_binary(v)->String.to_charlist(v)
      %{"ok"=>%{"kind"=>"type","value"=>v}}->throw({:unsupported_bridge_getter_native,v})
      %{"error"=>v}->throw({:rust_bridge_error,v})
      _->throw({:unsupported_bridge_reply,reply})
    end
  end
end

defmodule SuiteBridge do
  def rewrite({:attribute,a,:module,:erl_types_SUITE}),do: {:attribute,a,:module,:erl_types_rust_suite}
  def rewrite({:call,a,{:remote,b,{:atom,c,:erl_types},{:atom,d,name}},args}) do
    rewritten=Enum.map(args,&rewrite/1)
    list=Enum.reduce(Enum.reverse(rewritten),{nil,a},fn arg,tail->{:cons,a,arg,tail}end)
    {:call,a,{:remote,b,{:atom,c,RustTypeBridge},{:atom,d,:call}},[{:atom,d,name},list]}
  end
  def rewrite(t) when is_tuple(t),do: t |> Tuple.to_list() |> Enum.map(&rewrite/1) |> List.to_tuple()
  def rewrite(t) when is_list(t),do: Enum.map(t,&rewrite/1)
  def rewrite(t),do: t
  def load(forms) do
    case :compile.forms(forms,[:binary,:return_errors,:return_warnings]) do
      {:ok,module,beam,_}->:code.load_binary(module,~c"rust suite bridge",beam)
      {:error,errors,warnings}->raise inspect({errors,warnings})
    end
  end
  def run(otp,binary,out) do
    File.mkdir_p!(out);suite=Path.join(otp,"lib/dialyzer/test/erl_types_SUITE.erl")
    {:ok,forms}=:epp.parse_file(String.to_charlist(suite),[],[])
    load(forms);load(Enum.map(forms,&rewrite/1))
    port=Port.open({:spawn_executable,String.to_charlist(binary)},[:binary,:exit_status,{:line,10000000}]);Process.put(:rust_port,port)
    cases=Enum.map([:consistency_and_to_string,:misc,:map_multiple_representations],fn name ->
      native=try do {:ok,apply(:erl_types_SUITE,name,[[]])}catch k,r->{:error,k,inspect(r)}end
      Process.put(:bridge_counts,%{})
      rust=try do {:ok,apply(:erl_types_rust_suite,name,[[]])}catch k,r->{:error,k,inspect(r,limit: :infinity)}end
      counts=Process.get(:bridge_counts);IO.puts("#{name}: #{inspect(rust)} (#{Enum.sum(Map.values(counts))} Rust calls)")
      %{case: name,native: inspect(native),rust: inspect(rust),passed: match?({:ok,_},rust)&&native==rust,calls: counts}
    end)
    Port.close(port)
    report=%{otp_sha: "ab489c0b260c637a9487f8753909ea94dacbbec2",cases: cases,passed: Enum.count(cases,&(&1.passed)),requested: length(cases),original_suite: suite,native_algebra_fallback: false,bridge_unsupported: Enum.flat_map(cases,fn c->if String.contains?(c.rust,"unsupported"),do: [c.rust],else: [] end),runtime: %{otp: to_string(:erlang.system_info(:otp_release)),erts: to_string(:erlang.system_info(:version))}}
    File.write!(Path.join(out,"report.json"),:json.encode(report));if report.passed != report.requested,do: System.halt(1)
  end
end
[otp,binary,out]=System.argv();SuiteBridge.run(otp,binary,out)
