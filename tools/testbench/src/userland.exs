# The userland disk's modules (userland.rs): writes into OUT, stripped by `beam_lib`, each module
# of every application named `app:NAME`, with the application's resource as NAME.app, and each
# module named `module:NAME`, as MODULE.beam. With "docs" the `Docs` chunk is kept as well; with
# "nodocs" it goes with the rest. Modules are found on the code path.
#
#   elixir [-pa EBIN ...] userland.exs OUT docs|nodocs app:APP ... module:MODULE ...
[out, docs | items] = System.argv()
keep = if docs == "docs", do: [~c"Docs"], else: []

strip = fn module, beam ->
  {:ok, {^module, stripped}} = :beam_lib.strip(beam, keep)
  # `beam_lib` gzips what it strips; the VM's loader reads a module's chunks as they are.
  File.write!(Path.join(out, "#{module}.beam"), :zlib.gunzip(stripped))
end

for item <- items do
  case String.split(item, ":", parts: 2) do
    ["app", app] ->
      app = String.to_atom(app)

      ebin =
        case :code.lib_dir(app) do
          {:error, _} -> raise "no application #{app} on the code path"
          dir -> Path.join(dir, "ebin")
        end

      resource = Path.join(ebin, "#{app}.app")
      {:ok, [{:application, ^app, props}]} = :file.consult(resource)
      File.cp!(resource, Path.join(out, "#{app}.app"))

      for module <- Keyword.fetch!(props, :modules) do
        strip.(module, File.read!(Path.join(ebin, "#{module}.beam")))
      end

    ["module", module] ->
      module = String.to_atom(module)

      case :code.which(module) do
        path when is_list(path) -> strip.(module, File.read!(path))
        _ -> raise "no module #{module} on the code path"
      end
  end
end
