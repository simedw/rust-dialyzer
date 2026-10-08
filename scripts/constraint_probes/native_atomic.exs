[root,commands,out]=System.argv()
Code.require_file(Path.join(root,"scripts/type_codec.exs"))
Code.require_file(Path.join(root,"scripts/type_wire.exs"))
src=Path.join(root,".cache/upstream/otp/lib/dialyzer/src/dialyzer_typesig.erl")
copy=Path.join(Path.dirname(out),"native_typesig_atomic_probe.erl")
File.write!(copy,File.read!(src)|>String.replace("-module(dialyzer_typesig).","-module(native_typesig_atomic_probe).\n-compile(export_all)."))
{:ok,:native_typesig_atomic_probe,beam}=:compile.file(String.to_charlist(copy),[:binary,{:i,String.to_charlist(Path.dirname(src))},:nowarn_export_all])
{:module,:native_typesig_atomic_probe}=:code.load_binary(:native_typesig_atomic_probe,String.to_charlist(copy),beam)
key=fn id-> if id["atom"],do: String.to_atom(id["atom"]),else: String.to_integer(id["integer"]) end
id=fn x->if is_atom(x),do: %{atom: Atom.to_string(x)},else: %{integer: Integer.to_string(x)} end
results=commands|>File.read!()|>String.split("\n",trim: true)|>Enum.map(fn line->
 c=:json.decode(line);lhs=TypeCodec.type(c["left"]);rhs=TypeCodec.type(c["right"]);map=Map.new(c["bindings"],fn e->{key.(e["id"]),TypeCodec.type(e["type"])}end)
 try do
  case :native_typesig_atomic_probe.solve_one_c({:constraint,lhs,String.to_atom(c["relation"]),rhs,[]},map) do
   :error->%{status: "failed"}
   {:ok,{map,updated}}->%{status: "solved",bindings: Enum.map(Enum.sort(map),fn {k,v}->%{id: id.(k),type: NativeTypeWire.encode(v)}end),updated: Enum.map(updated,id)}
  end
 catch kind,reason->
   {class,payload}=case reason do
    {name,v} when name in [:case_clause,:badmatch,:bad_generator]->{name,if(is_boolean(v),do: v,else: NativeTypeWire.encode(v))}
    {:bad_generators,{left,right}}->{:bad_generators,[Enum.map(left,&NativeTypeWire.encode/1),Enum.map(right,&NativeTypeWire.encode/1)]}
    name when is_atom(name)->{name,:null}
   end
   %{status: "native_error",class: Atom.to_string(class),payload: payload,detail: inspect({kind,reason})}
  end
end)
File.write!(out,:json.encode(results))
