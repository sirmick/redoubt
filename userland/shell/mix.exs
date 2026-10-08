defmodule Redoubt.Shell.MixProject do
  use Mix.Project

  def project do
    [
      app: :redoubt_shell,
      version: "0.1.0",
      elixir: "~> 1.20",
      deps: [],
      # The commandlets' index is written after the Elixir compiler, and before the application's
      # resource, which lists it with the other modules.
      compilers: [:erlang, :elixir, :commandlets, :app],
      # The wire's Elixir codecs and their generated clients (docs/servers/wire.md, "Generated
      # clients") are part of the shell's application: a session binds every server through them.
      elixirc_paths: [
        "lib" | Enum.map(~w(lib proto client), &Path.expand("../../libs/wire/elixir/" <> &1, __DIR__))
      ],
      # test/support holds what the tests share (the terminal model); test_helper requires it.
      test_ignore_filters: [~r"^test/support/"],
      # beamlet looks a module up in the system's code before any other directory, so a
      # protocol consolidated here would never be the one used on Redoubt. Build as it runs.
      consolidate_protocols: false
    ]
  end

  # crypto for checksum: its natives are beamlet-crypto's on beamlet.
  def application, do: [extra_applications: [:crypto]]
end

defmodule Mix.Tasks.Compile.Commandlets do
  @moduledoc false
  # Writes `Redoubt.Commandlet.Index` (Redoubt.Commandlet.Registry): the modules of the
  # application that declare commands, found here by the exports in their .beam files, and a
  # function for each command that calls its module's. The prompt imports the index alone, so the
  # shell starts without loading a module to ask whether it has commands, and a command's module
  # is loaded when the command is first called.

  use Mix.Task.Compiler

  @index Redoubt.Commandlet.Index

  @impl true
  def run(_args) do
    ebin = Mix.Project.compile_path()
    target = Path.join(ebin, "#{@index}.beam")

    modules =
      for beam <- ebin |> Path.join("*.beam") |> Path.wildcard() |> Enum.sort(),
          beam != target,
          {:ok, {module, [exports: exports]}} <- [:beam_lib.chunks(String.to_charlist(beam), [:exports])],
          {:__commandlets__, 0} in exports,
          do: module

    commands = for module <- modules, command <- module.__commandlets__(), do: command
    {_module, binary} = compile(modules, commands)

    # Elixir's type-checker chunk differs from one compile to the next; the rest is the same.
    if File.exists?(target) and stripped(File.read!(target)) == stripped(binary) do
      {:noop, []}
    else
      File.write!(target, binary)
      {:ok, []}
    end
  end

  defp stripped(beam) do
    {:ok, {_module, stripped}} = :beam_lib.strip(beam)
    stripped
  end

  defp compile(modules, commands) do
    imports = Enum.flat_map(commands, &Redoubt.Commandlet.arities/1)
    by_name = Map.new(commands, &{Atom.to_string(&1.name), &1.module})

    calls =
      for command <- commands, {name, arity} <- Redoubt.Commandlet.arities(command) do
        args = Macro.generate_arguments(arity, __MODULE__)

        quote do:
                def(unquote(name)(unquote_splicing(args)),
                  do: unquote(command.module).unquote(name)(unquote_splicing(args))
                )
      end

    quoted =
      quote do
        defmodule unquote(@index) do
          @moduledoc false
          def __modules__, do: unquote(modules)
          def __imports__, do: unquote(imports)
          def __commands__, do: unquote(Macro.escape(by_name))
          unquote_splicing(calls)
        end
      end

    # Compiled here, in the build's VM, where an earlier index may be loaded already.
    conflicts = Code.get_compiler_option(:ignore_module_conflict)
    Code.put_compiler_option(:ignore_module_conflict, true)

    try do
      [compiled] = Code.compile_quoted(quoted, "commandlets")
      compiled
    after
      Code.put_compiler_option(:ignore_module_conflict, conflicts)
    end
  end
end
