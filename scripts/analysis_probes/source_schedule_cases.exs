Code.require_file(System.get_env("DIALYZER_CORE_EXPORT") || "/Users/simedw/Documents/ChatGPT/rust-dialyzer/scripts/core_export.exs")
:logger.set_primary_config(:level,:none)
[out]=System.argv();File.mkdir_p!(out)
lit=&:cerl.abstract/1
for count<-[0,1,2,31,32,33,64,257,513],reverse<-[false,true],kind<-0..2 do
 names=if count==0,do: [],else: Enum.map(0..(count-1),&String.to_atom("schedule_#{&1}"))
 mods=Enum.with_index(names)|>Enum.map(fn {name,index}->
  x=:cerl.c_var(:x);entry=:cerl.c_var({:entry,1})
  targets=case kind do
   0->Enum.take(Enum.drop(names,index+1),3)
   1->if count==0,do: [],else: Enum.map([1,3,17],&Enum.at(names,rem(index+&1,count)))
   2->Enum.take(names,5)
  end
  calls=Enum.map(targets,fn target->:cerl.c_call(lit.(target),lit.(:entry),[x]) end)
  nested=:cerl.c_fun([],:cerl.c_tuple_skel(calls))
  body=:cerl.c_tuple_skel(calls++[nested,:cerl.c_apply(entry,[x])])
  tree=:cerl.c_module(lit.(name),[entry],[],[{entry,:cerl.c_fun([x],body)}])
  CoreExport.export_module(name,[],tree)
 end)
 mods=if reverse,do: Enum.reverse(mods),else: mods
 path=Path.expand(Path.join(out,"#{count}-#{reverse}-#{kind}.json"))
 File.write!(path,:json.encode(%{version: 6,profile: CoreExport.profile(),atom_layout: CoreExport.atom_layout(mods),modules: mods}))
 IO.puts(:json.encode(%{id: "scheduler/#{count}/#{reverse}/#{kind}",core_json: path}))
end
