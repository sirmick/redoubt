defmodule Redoubt.Commandlet.RegistryTest do
  use ExUnit.Case, async: true

  alias Redoubt.Commandlet
  alias Redoubt.Commandlet.Registry

  test "every command of the shell is found, and each has a name of its own" do
    names = Enum.map(Registry.all(), & &1.name)
    assert names == Enum.uniq(names)
    assert Enum.sort([:cat, :cd, :count, :cp, :grep, :head, :help, :ls, :mv, :pwd]) -- names == []
  end

  test "every command's page renders" do
    for cmd <- Registry.all() do
      assert [usage, "", summary | _rest] = Commandlet.page(cmd)
      assert usage == Commandlet.usage(cmd)
      assert summary == cmd.summary
    end
  end

  test "a command is found by atom or by string, and a name looked up never becomes an atom" do
    assert {:ok, %Commandlet{name: :grep}} = Registry.fetch(:grep)
    assert {:ok, %Commandlet{name: :grep}} = Registry.fetch("grep")

    name = "redoubt_never_a_command_#{System.unique_integer([:positive])}"
    assert Registry.fetch(name) == :error
    assert_raise ArgumentError, fn -> String.to_existing_atom(name) end
  end

  test "the prompt imports each command, every arity of it, from the index, and nothing else" do
    {:import, _meta, [Redoubt.Commandlet.Index, [only: only]]} = Registry.imports()
    commands = MapSet.new(Registry.all(), & &1.name)

    assert [help: 0, help: 1, h: 1, grep: 2, grep: 3, head: 1, head: 2, cat: 1] -- only == []
    assert Enum.sort(only) == Enum.sort(Enum.flat_map(Registry.all(), &Commandlet.arities/1))

    for {name, _arity} <- only,
        do: assert(name in commands, "#{name} is imported but is not a command")
  end

  test "the index finds every module that declares commands, and calls the command's own function" do
    # Loading is idempotent, and makes the module list known where nothing has started the
    # application, as under beamlet's test runner.
    _ = Application.load(:redoubt_shell)

    declaring =
      for module <- Application.spec(:redoubt_shell, :modules),
          Code.ensure_loaded?(module),
          function_exported?(module, :__commandlets__, 0),
          do: module

    assert Enum.sort(Registry.modules()) == Enum.sort(declaring)
    assert Redoubt.Commandlet.Index.pwd() == Redoubt.Shell.Helpers.pwd()
  end

  test "help lists every command by area, and shows one command's page" do
    index = Enum.to_list(Redoubt.Shell.Help.help())
    assert "Files" in index and "Text" in index and "Shell" in index
    assert Enum.any?(index, &(&1 =~ ~r/^  cp +Copy a file$/))

    assert Enum.to_list(Redoubt.Shell.Help.help(:cp)) == Commandlet.page(elem(Registry.fetch(:cp), 1))
    assert Enum.to_list(Redoubt.Shell.Help.help("cp")) == Enum.to_list(Redoubt.Shell.Help.help(:cp))
  end
end
