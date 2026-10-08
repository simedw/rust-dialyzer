Code.require_file("native_types.exs",__DIR__)
[source,output]=System.argv()
original=source|>File.read!()|>:json.decode()
var=:erl_types.t_var(7)
other=:erl_types.t_var(:R)
integer=:erl_types.t_integer()
range=:erl_types.t_from_range(1,3)
nominal=:erl_types.t_nominal({:sample,:nominal,0,:opaque},other)
signature=:erl_types.t_fun([var],:erl_types.t_tuple([var,nominal]))
make=fn alternatives->
  pairs=Enum.map(alternatives,fn {signature,constraints}->%{signature: NativeTypes.term(signature),constraints: Enum.map(constraints,fn {left,right}->%{left: NativeTypes.term(left),right: NativeTypes.term(right)}end)}end)
  general=Enum.reduce(alternatives,:erl_types.t_none(),fn {signature,constraints},acc->
    bounds=Enum.reduce(constraints,%{},fn {left,right},acc->name=:erl_types.t_var_name(left);Map.update(acc,name,right,&:erl_types.t_inf(&1,right))end)
    :erl_types.t_sup(acc,:erl_types.t_subst(signature,bounds))
  end)|>:erl_types.subst_all_vars_to_any()
  %{alternatives: pairs,arguments: Enum.map(:erl_types.t_fun_args(general),&NativeTypes.term/1)}
end
extra=[%{mfa: %{module: "synthetic",function: "integer_variable",arity: 1},contract: make.([{signature,[{var,integer},{var,range},{other,:erl_types.t_atom(:ok)}]}])},
%{mfa: %{module: "synthetic",function: "overloaded_nominal",arity: 1},contract: make.([{:erl_types.t_fun([:erl_types.t_atom(:left)],:erl_types.t_integer(1)),[]},{:erl_types.t_fun([:erl_types.t_atom(:right)],:erl_types.t_nominal(:brand,:erl_types.t_list(integer))),[]}])}]
File.write!(output,:json.encode(%{contracts: original["contracts"]++extra}))
