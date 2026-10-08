{:ok,m,bin}=:compile.file(String.to_charlist(Path.join(Path.dirname(out),"native_typesig_tree_probe.erl")),[:binary,:nowarn_export_all,{:i,'/Users/simedw/Documents/ChatGPT/rust-dialyzer/.cache/upstream/otp/lib/dialyzer/src'}])
:code.load_binary(m,~c"raw_probe",bin)
for type <- [:erl_types.t_any(),:erl_types.t_atom(:a),:erl_types.t_var(200)] do
 f={:fun_var,fn _->type end,[],:undefined}
 c={:constraint,:erl_types.t_var(1),:eq,f,[]}
 result=try do :native_typesig_tree_probe.solve_one_c(c,%{}) catch kind,reason->{kind,reason} end
 IO.inspect({type,result})
end
