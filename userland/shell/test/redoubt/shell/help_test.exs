defmodule Redoubt.Shell.HelpTest do
  use ExUnit.Case, async: true

  import Redoubt.Shell.Help

  alias Redoubt.Commandlet.{Registry, UsageError}
  alias Redoubt.Shell.Topics

  defp lines(value), do: Enum.to_list(value)

  test "help(:topic) shows a topic, and help() ends by naming them" do
    assert ["# Elixir at the prompt" | _] = lines(help(:elixir))
    assert ["# What the terminal shows, and why" | _] = lines(help("terminal"))
    assert "Topics: help(:elixir), help(:terminal)" in lines(help())
  end

  test "each topic starts with its title, and none has a command's name" do
    commands = MapSet.new(Registry.all(), &Atom.to_string(&1.name))

    for {name, title} <- Topics.all() do
      assert title != "", "#{name}.md does not start with a # title"
      refute name in commands, "the topic #{name} has a command's name"
    end
  end

  test "help of neither a command nor a topic is refused, and makes no atom" do
    assert_raise UsageError, ~r/help: there is no command or topic named nope; help\(\) lists them/, fn ->
      help(:nope)
    end

    name = "redoubt_never_a_topic_#{System.unique_integer([:positive])}"
    assert_raise UsageError, fn -> help(name) end
    assert_raise ArgumentError, fn -> String.to_existing_atom(name) end
  end

  test "h shows a module's documentation, or a function's at any arity its defaults allow" do
    assert ["# File", "" | _] = lines(h(File))

    for fun <- [&File.cp/2, &File.cp/3] do
      assert ["File.cp(source_file, destination_file, options \\\\ [])", "" | doc] = lines(h(fun))
      assert Enum.any?(doc, &(&1 =~ "Copies the contents"))
    end

    assert ["Redoubt.Util.grep(lines, pattern, opts \\\\ [])", "", "Keep the lines that match a pattern." | _] =
             lines(h(&Redoubt.Util.grep/2))
  end

  test "h says when there is nothing to show, and refuses what is not a module or a function" do
    # Kernel.Utils is Elixir's own and always there; IEx is not on beamlet's code path, as the shell does not use it.
    assert lines(h(Kernel.Utils)) == ["Kernel.Utils is internal: it has no documentation"]

    assert_raise UsageError, ~r/there is no module :redoubt_no_such_module/, fn ->
      h(:redoubt_no_such_module)
    end

    assert lines(h(&Redoubt.Shell.Help.__commandlets__/0)) == [
             "Redoubt.Shell.Help.__commandlets__/0 is internal: it has no documentation"
           ]

    assert_raise UsageError, ~r/an anonymous function has no documentation/, fn -> h(fn x -> x end) end
    assert_raise UsageError, ~r/target must be a module, or a function reference/, fn -> h("File") end
  end
end
