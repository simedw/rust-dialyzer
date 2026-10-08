{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_dataflow),[:abstract_code])
forms=Enum.map(forms,fn {:attribute,a,:module,_}->{:attribute,a,:module,:rust_state_native};f->f end)
{:ok,:rust_state_native,binary,_}=:compile.forms(forms,[:binary,:export_all,:return_warnings])
{:module,:rust_state_native}=:code.load_binary(:rust_state_native,~c"state_oracle",binary)
key=fn "top"->:top;n->n end
for line<-IO.stream(:stdio,:line) do
 row=:json.decode(line);dict=Enum.reduce(row["insert"],:dict.new(),fn k,d->:dict.store(key.(k),true,d) end)
 work=:rust_state_native.init_work(Enum.map(row["initial"],key))
 {steps,_}=Enum.map_reduce(row["operations"],work,fn op,work->
  if op=="pop" do
   case :rust_state_native.get_work(work) do
    :none->{%{result: :null,queue: []},work}
    {result,new}->{%{result: result,queue: elem(new,0)++Enum.reverse(elem(new,1))},new}
   end
  else
   new=:rust_state_native.add_work(key.(op["add"]),work);{%{queue: elem(new,0)++Enum.reverse(elem(new,1))},new}
  end
 end)
 IO.puts(:json.encode(%{id: row["id"],dict: :dict.fetch_keys(dict),slots: [elem(dict,2),elem(dict,3),elem(dict,4)],steps: steps}))
end
