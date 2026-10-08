defmodule Redoubt.Shell.CompleterTest do
  # Path completion reads the VM's working directory, which the tests share: run alone.
  use ExUnit.Case, async: false

  alias Redoubt.Shell.Completer

  @moduletag :tmp_dir

  @context %{
    vars: ["files", "total"],
    imports: ["cat", "cd", "cp", "count", "help"],
    aliases: %{"Str" => String}
  }

  setup %{tmp_dir: dir} do
    here = File.cwd!()
    File.cd!(dir)
    on_exit(fn -> File.cd!(here) end)
    for name <- ["notes.txt", "notes.md", "other.txt", ".hidden"], do: File.write!(name, "")
    File.mkdir_p!("logs/old")
    File.write!("logs/app.log", "")
    :ok
  end

  defp tab(before, context \\ @context), do: Completer.expand(before, context)

  test "a name completes from the variables and the imports; a function alone gets its parenthesis" do
    assert tab("hel") == {:yes, ~c"p(", []}
    assert tab("tot") == {:yes, ~c"al", []}
    assert tab("x = fi") == {:yes, ~c"les", []}
    # Several: what they share is inserted, and they are listed.
    assert tab("c") == {:yes, [], [~c"cat", ~c"cd", ~c"count", ~c"cp"]}
    assert tab("co") == {:yes, ~c"unt(", []}
    assert tab("zz") == {:no, [], []}
  end

  test "the prompt's context comes from its binding and its environment" do
    env = Code.env_for_eval(file: "shell")

    {_, _, env} =
      Code.eval_quoted_with_env(
        Code.string_to_quoted!("import Redoubt.Shell.Helpers; alias Redoubt.Util.Lines, as: L"),
        [],
        env
      )

    context = Completer.context([mine: 1], env)
    assert "mine" in context.vars
    assert "ls" in context.imports
    assert "is_atom" in context.imports
    assert context.aliases["L"] == Redoubt.Util.Lines
  end

  test "a module's functions complete after its name and a dot, through an alias too" do
    assert tab("File.exi") == {:yes, ~c"sts?(", []}
    assert tab("Str.trim_l") == {:yes, ~c"eading(", []}
    assert tab(":lists.reve") == {:yes, ~c"rse(", []}
    assert tab("NoSuchModule.x") == {:no, [], []}
    # A name typed never becomes an atom.
    assert tab(":no_such_module_typed_here.x") == {:no, [], []}
    assert_raise ArgumentError, fn -> String.to_existing_atom("no_such_module_typed_here") end
  end

  test "completing never makes an atom: nothing typed is parsed" do
    probes = [
      ~S|zqxnosuchcmd9("no|,
      ~S{zqxnosuchvar8 |> cat("oth},
      ~S|zqxnosuchcmd7(:hexd|,
      "\u00F1cat(\"oth",
      "\u00D1cat.x(\"oth",
      ~S|help(:'cat cd', "oth|,
      ~S|:'cat-cd'|,
      ~S|cat(~z"", "oth|,
      ~S|cat(~CAT"", "oth|
    ]

    # The same paths first with names that are atoms already, so a module they load (and its own
    # atoms) is loaded before counting.
    for warm <- [~S|cat("oth|, ~S|help(:hexd|, ~S|cat.x("oth|, ~S|cat(~s"", "oth|, ~S|:'cat'|] do
      tab(warm)
    end

    for probe <- probes do
      before = :erlang.system_info(:atom_count)
      tab(probe)
      assert :erlang.system_info(:atom_count) == before, "Tab after #{probe} made an atom"
    end

    assert tab(~S|zqxnosuchcmd9("no|) == {:no, [], []}
    assert tab(~S{zqxnosuchvar8 |> cat("oth}) == {:yes, ~c"er.txt", []}
    # A name that only ends like a command's is not that command.
    assert tab("\u00F1cat(\"oth") == {:no, [], []}
    assert tab(~S|File.cat("oth|) == {:no, [], []}
    # Inside a list, the argument is not the command's own.
    assert tab(~S|cat(["oth|) == {:no, [], []}
    # `?(` is a character, not a bracket: the string is still cat's, its second path.
    assert tab(~S|cat(?(, "oth|) == {:yes, ~c"er.txt", []}

    # A closed string's words are not names: they are left out before parsing.
    assert tab(~S|cp("zqx unknown words", "oth|) == {:yes, ~c"er.txt", []}
  end

  test "a module's name completes a segment at a time" do
    # The completer's own module is loaded, so listed on either VM.
    {:yes, ~c"leter", _candidates} = tab("Redoubt.Shell.Comp")
    # Redoubt.Shell and Redoubt.Shell.Completer are one segment here.
    {:yes, ~c"l", candidates} = tab("Redoubt.Shel")
    refute ~c"Redoubt.Shell.Completer" in candidates
  end

  test "inside a string given to a path, the directory's names complete, a directory with a slash" do
    assert tab(~S|cat("oth|) == {:yes, ~c"er.txt", []}
    assert tab(~S|cat("no|) == {:yes, ~c"tes.", [~c"notes.md", ~c"notes.txt"]}
    assert tab(~S|cd("lo|) == {:yes, ~c"gs/", []}
    assert tab(~S|cat("logs/a|) == {:yes, ~c"pp.log", []}
    assert tab(~S|cp("other.txt", "logs/o|) == {:yes, ~c"ld/", []}
    # Piped: the pipe gives the first argument.
    assert tab(~S{files |> cp("logs/o}) == {:yes, ~c"ld/", []}
  end

  test "a hidden name is offered only when its dot is typed" do
    {:yes, _insert, candidates} = tab(~S|cat("|)
    refute ~c".hidden" in candidates
    assert tab(~S|cat(".h|) == {:yes, ~c"idden", []}
  end

  test "a string that is not a path is not completed as one; a closed string is code again" do
    assert tab(~S|grep(lines, "no|) == {:no, [], []}
    assert tab(~S|IO.puts("no|) == {:no, [], []}
    assert tab(~S{cat("notes.txt") |> cou}) == {:yes, ~c"nt(", []}
    # An escaped quote does not close the string, and a ?" is no quote.
    assert tab(~S|cat("a\"b|) == {:no, [], []}
    assert tab(~S|x = ?"; tot|) == {:yes, ~c"al", []}
  end

  test "help's subject completes from the commands and topics, as a string or an atom" do
    {:yes, ~c"p", candidates} = tab(~S|help("gre|)
    assert ~c"grep" in candidates and ~c"grep_v" in candidates
    assert tab(~S|help("hexd|) == {:yes, ~c"ump", []}
    assert tab(~S|help(:hexd|) == {:yes, ~c"ump", []}
    assert tab(~S|help(:elix|) == {:yes, ~c"ir", []}
  end

  test "what group calls takes the line reversed, as group keeps it" do
    env = Code.env_for_eval(file: "shell")
    assert Completer.tab(Enum.reverse(~c"tot"), [:total], env.functions, env.aliases) == {:yes, ~c"al", []}
  end
end
