[root, paths_file] = System.argv()
Code.require_file("core_export.exs", Path.join(root, "scripts"))
paths = File.read!(paths_file) |> String.split("\n", trim: true)
Enum.each(paths, &CoreExport.module/1)
