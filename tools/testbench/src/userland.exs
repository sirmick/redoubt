# The userland disk's modules (userland.rs): writes into OUT each module of every application
# named after it, stripped by `beam_lib`, as MODULE.beam, and its application resource as APP.app.
# With "docs" the `Docs` chunk is kept as well; with "nodocs" it goes with the rest.
#
#   elixir [-pa EBIN ...] userland.exs OUT docs|nodocs APP ...
[out, docs | apps] = System.argv()
keep = if docs == "docs", do: [~c"Docs"], else: []

for app <- Enum.map(apps, &String.to_atom/1) do
  ebin =
    case :code.lib_dir(app) do
      {:error, _} -> raise "no application #{app} on the code path"
      dir -> Path.join(dir, "ebin")
    end

  resource = Path.join(ebin, "#{app}.app")
  {:ok, [{:application, ^app, props}]} = :file.consult(resource)
  File.cp!(resource, Path.join(out, "#{app}.app"))

  for module <- Keyword.fetch!(props, :modules) do
    beam = File.read!(Path.join(ebin, "#{module}.beam"))
    {:ok, {^module, stripped}} = :beam_lib.strip(beam, keep)
    # `beam_lib` gzips what it strips; the VM's loader reads a module's chunks as they are.
    File.write!(Path.join(out, "#{module}.beam"), :zlib.gunzip(stripped))
  end
end
