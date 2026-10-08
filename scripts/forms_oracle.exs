Code.require_file("forms_runtime.exs",__DIR__)
for line<-IO.stream(:stdio,:line) do
 request=:json.decode(line)
 IO.puts(:json.encode(FormsOracle.run(request)))
end
