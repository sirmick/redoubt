defmodule Redoubt.CommandletTest do
  use ExUnit.Case, async: true

  alias Redoubt.Commandlet
  alias Redoubt.Commandlet.{Param, UsageError}

  # Not in the shell's application, so in no registry: a command for these tests alone.
  defmodule Sample do
    use Redoubt.Commandlet, area: "Sample"

    @summary "Take a name, a count and files"
    @help "Returns what it was given, as the body got it."
    @args name: "a name", times: "how many times", files: "the files"
    @examples [{~S'take("x", 2, ["a", "b"])', "x twice, and two files"}]
    defcommand take(name :: string, times :: integer(min: 1, max: 3) \\ 1, files :: many(path) \\ []) do
      {name, times, files}
    end
  end

  defmodule Options do
    use Redoubt.Commandlet, area: "Sample"

    @summary "Take files, and options"
    @help "Returns what it was given, as the body got it."
    @args files: "the files", reverse: "backwards", limit: "at most this many", mode: "how"
    @examples [{~S'opts(["a"], reverse: true, mode: :fast)', "a, backwards and fast"}]
    defcommand opts(
                 files :: many(path),
                 flags :: flags(reverse: boolean, limit: integer(min: 1), mode: one_of([:fast, :slow]))
               ) do
      {files, flags}
    end
  end

  defp refusal(fun) do
    error = assert_raise UsageError, fun
    error.message
  end

  defp sample, do: hd(Sample.__commandlets__())

  test "arguments that fit their types reach the body, coerced where the type says" do
    assert Sample.take("x") == {"x", 1, []}
    assert Sample.take("x", 3, "a.txt") == {"x", 3, ["a.txt"]}
    assert Sample.take("x", 2, ["a", "b"]) == {"x", 2, ["a", "b"]}
  end

  test "an argument that does not fit is refused, naming the parameter and showing the usage" do
    usage = ~S"usage: take(name, times \\ 1, files \\ [])"

    assert refusal(fn -> Sample.take(:x) end) == "take: name must be a string, got :x\n" <> usage
    assert refusal(fn -> Sample.take("x", 0) end) =~ "take: times must be an integer 1..3, got 0\n"
    assert refusal(fn -> Sample.take("x", 1, [1]) end) =~ "files must be a path, or a list of them, got [1]"
    assert refusal(fn -> Sample.take("x", 1, "") end) =~ ~S(files must be a path, or a list of them, got "")
    assert refusal(fn -> Sample.take("x", 1, "a\0b") end) =~ "a path holds no NUL"
  end

  test "lines refuse a string, and say how to get lines" do
    assert refusal(fn -> Redoubt.Util.grep("app.log", "x") end) =~
             ~S[lines must be an enumerable of lines, got "app.log"; a string is not lines: cat(path)]

    assert refusal(fn -> Redoubt.Util.count(42) end) =~ "lines must be an enumerable of lines, got 42"
  end

  test "a command parameter holds the commandlet it names" do
    param = %Param{name: :command, type: {:command, []}}
    assert {:ok, %Commandlet{name: :cp, module: Redoubt.Shell.Helpers}} = Param.check(param, "cp")
    assert {:ok, %Commandlet{name: :cp}} = Param.check(param, :cp)
    assert {:error, "there is no command named nope; help() lists them"} = Param.check(param, "nope")
  end

  test "flags are a keyword list, and the body gets every flag, given or not" do
    assert Options.opts("a") == {["a"], %{reverse: false, limit: nil, mode: nil}}

    assert Options.opts(["a", "b"], reverse: true, limit: 2, mode: "slow") ==
             {["a", "b"], %{reverse: true, limit: 2, mode: :slow}}
  end

  test "a wrong flag is refused by name" do
    usage = ~S"usage: opts(files, flags \\ [])"

    assert refusal(fn -> Options.opts("a", backwards: true) end) ==
             "opts: there is no option backwards; the options are reverse, limit, mode\n" <> usage

    assert refusal(fn -> Options.opts("a", limit: 0) end) =~
             "opts: the option limit must be an integer >= 1, got 0\n"

    assert refusal(fn -> Options.opts("a", reverse: 1) end) =~
             "the option reverse must be true or false, got 1"

    assert refusal(fn -> Options.opts("a", mode: :medium) end) =~
             "the option mode must be one of :fast, :slow"

    assert refusal(fn -> Options.opts("a", reverse: true, reverse: false) end) =~
             "the option reverse is given twice"

    assert refusal(fn -> Options.opts("a", :reverse) end) =~ "flags must be options: reverse: boolean"
  end

  test "one_of takes a string naming one, and a string naming none never becomes an atom" do
    param = %Param{name: :mode, type: {:one_of, [:fast, :slow]}}
    assert Param.check(param, "fast") == {:ok, :fast}
    assert Param.check(param, :slow) == {:ok, :slow}

    name = "redoubt_never_a_mode_#{System.unique_integer([:positive])}"
    assert Param.check(param, name) == {:error, nil}
    assert_raise ArgumentError, fn -> String.to_existing_atom(name) end
  end

  test "a ref is a module or a named function" do
    param = %Param{name: :target, type: {:ref, []}}
    assert Param.check(param, File) == {:ok, {:module, File}}
    assert Param.check(param, &File.cp/2) == {:ok, {:function, File, :cp, 2}}
    assert {:error, "an anonymous function has no documentation" <> _} = Param.check(param, fn x -> x end)

    assert {:error, "there is no module :redoubt_no_such_module"} =
             Param.check(param, :redoubt_no_such_module)
  end

  test "a page lists each flag as it is given" do
    page = Commandlet.page(hd(Options.__commandlets__()))
    assert "  files     path | [path]  the files" in page
    assert "  reverse:  boolean        backwards" in page
    assert "  mode:     fast | slow    how" in page
  end

  # Compiles a module holding `body`, which declares a command.
  defp declare(body) do
    Code.compile_string("""
    defmodule Redoubt.CommandletTest.Bad#{System.unique_integer([:positive])} do
      use Redoubt.Commandlet, area: "Test"
    #{body}
    end
    """)
  end

  @help_block ~S'''
  @summary "Do it"
  @help "It does it."
  @args path: "a file"
  @examples [{~S|go("a")|, "do it to a"}]
  '''

  test "a command without its help does not compile" do
    without = fn attribute -> String.replace(@help_block, ~r/^@#{attribute} .*$/m, "") end
    command = "defcommand go(path :: path), do: path"

    assert_raise CompileError, ~r/go: no @summary/, fn -> declare(without.("summary") <> command) end
    assert_raise CompileError, ~r/go: no @help/, fn -> declare(without.("help") <> command) end
    assert_raise CompileError, ~r/go: no @args line for path/, fn -> declare(without.("args") <> command) end
    assert_raise CompileError, ~r/go: no @examples/, fn -> declare(without.("examples") <> command) end

    assert_raise CompileError, ~r/the example "stop\(\)" does not call go/, fn ->
      declare(String.replace(@help_block, ~S|go("a")|, "stop()") <> command)
    end

    assert_raise CompileError, ~r/@args names other, which is not a parameter/, fn ->
      declare(String.replace(@help_block, ~S|path: "a file"|, ~S|path: "a file", other: "x"|) <> command)
    end
  end

  test "a signature that breaks the rules of parameters does not compile" do
    declare_as = fn signature -> declare(@help_block <> "defcommand #{signature}, do: :ok") end

    assert_raise CompileError, ~r/go: path: file is not a type; the types are path/, fn ->
      declare_as.("go(path :: file)")
    end

    assert_raise CompileError, ~r/integer has no option least/, fn ->
      declare_as.("go(path :: integer(least: 1))")
    end

    assert_raise CompileError, ~r/write path as `name :: type`/, fn -> declare_as.("go(path)") end

    assert_raise CompileError, ~r/parameters with a default come after those without/, fn ->
      declare_as.(~S|go(path :: path \\ "a", n :: integer)|)
    end

    assert_raise CompileError, ~r/only the last parameter, or the one before flags, may be many/, fn ->
      declare_as.("go(path :: many(path), n :: integer)")
    end

    assert_raise CompileError, ~r/only the first parameter may be lines/, fn ->
      declare_as.("go(path :: path, lines :: lines)")
    end

    assert_raise CompileError, ~r/only the last parameter may be flags/, fn ->
      declare_as.("go(o :: flags(all: boolean), path :: path)")
    end

    assert_raise CompileError, ~r/flag all: a flag cannot be lines/, fn ->
      declare_as.("go(path :: path, o :: flags(all: lines))")
    end

    assert_raise CompileError, ~r/two parameters or flags have one name/, fn ->
      declare_as.("go(path :: path, o :: flags(path: boolean))")
    end
  end

  test "each flag needs its @args line, and the flags parameter itself none" do
    command = "defcommand go(path :: path, o :: flags(all: boolean)), do: {path, o}"

    assert_raise CompileError, ~r/no @args line for all/, fn -> declare(@help_block <> command) end

    assert_raise CompileError, ~r/@args names o, which is not a parameter or a flag/, fn ->
      declare(
        String.replace(@help_block, ~S|path: "a file"|, ~S|path: "a file", all: "all", o: "x"|) <> command
      )
    end

    assert [{_module, _binary}] =
             declare(
               String.replace(@help_block, ~S|path: "a file"|, ~S|path: "a file", all: "all"|) <> command
             )
  end

  test "the page shows the usage, the summary, each parameter, the help and the examples" do
    page = Commandlet.page(sample())

    assert page == [
             ~S"take(name, times \\ 1, files \\ [])",
             "",
             "Take a name, a count and files",
             "",
             "  name   string         a name",
             "  times  integer 1..3   how many times",
             "  files  path | [path]  the files",
             "",
             "Returns what it was given, as the body got it.",
             "",
             "Examples",
             ~S'  take("x", 2, ["a", "b"])  x twice, and two files'
           ]
  end

  test "the help is also the function's documentation" do
    {:docs_v1, _anno, _language, _format, _module_doc, _meta, docs} = Code.fetch_docs(Redoubt.Util)
    [doc] = for {{:function, :grep, 3}, _anno, _signature, %{"en" => doc}, _meta} <- docs, do: doc

    assert doc =~ "Keep the lines that match a pattern.\n\nThe lines holding the pattern"
    assert doc =~ "* `pattern` (pattern): a string to find, or a regex to match"
    assert doc =~ ~S'## Examples' <> "\n\n" <> ~S'    cat("app.log") |> grep("error")'
  end
end
