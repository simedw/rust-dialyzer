[root,input,output]=System.argv()
for name<-[:file_utils,:dialyzer_common] do
 path=Path.join(root,".cache/upstream/otp/lib/dialyzer/test/"<>Atom.to_string(name)<>".erl")
 result=:compile.file(String.to_charlist(path),[:binary,:return_errors,:return_warnings])
 {^name,bytes}=case result do {:ok,m,b}->{m,b};{:ok,m,b,_}->{m,b} end
 {:module,^name}=:code.load_binary(name,String.to_charlist(path),bytes)
end
native=for line<-File.stream!(input) do
 row=:json.decode(line)
 missing=:dialyzer_common.new_tests(String.to_charlist(row["suite_dir"]),Enum.map(row["declared"],&String.to_atom/1))
 [:json.encode(%{id: row["id"],missing: Enum.map(missing,&Atom.to_string/1)}),"\n"]
end
File.write!(output,native)
