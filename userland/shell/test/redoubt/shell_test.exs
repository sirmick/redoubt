defmodule Redoubt.ShellTest do
  # The shell's helpers include cd, which moves the VM's working directory: run alone.
  use ExUnit.Case, async: false

  @moduletag :tmp_dir

  # Runs the shell on `input` as its console, and returns everything it wrote, prompts included.
  defp session(input, opts \\ []) do
    {:ok, console} = StringIO.open(input, capture_prompt: true)
    here = File.cwd!()

    task =
      Task.async(fn ->
        Process.group_leader(self(), console)
        Redoubt.Shell.run([banner: false] ++ opts)
      end)

    assert Task.await(task, 60_000) == :ok
    File.cd!(here)
    {_input, output} = StringIO.contents(console)
    output
  end

  # The shell over a console that answers each line request with the next of `lines`, as the
  # bytes they are, as beamlet's console does and StringIO will not, and keeps what is written.
  defp raw_session(lines) do
    console = spawn_link(fn -> console(lines, []) end)

    task =
      Task.async(fn ->
        Process.group_leader(self(), console)
        Redoubt.Shell.run(banner: false)
      end)

    assert Task.await(task, 60_000) == :ok
    send(console, {:written, self()})
    assert_receive {:written, output}
    output
  end

  defp console(lines, out) do
    receive do
      {:io_request, from, ref, {:get_line, _encoding, prompt}} ->
        {reply, rest} = if lines == [], do: {:eof, []}, else: {hd(lines), tl(lines)}
        send(from, {:io_reply, ref, reply})
        console(rest, [out, IO.chardata_to_string(prompt)])

      {:io_request, from, ref, {:put_chars, _encoding, chars}} ->
        send(from, {:io_reply, ref, :ok})
        console(lines, [out, IO.chardata_to_string(chars)])

      {:io_request, from, ref, {:put_chars, _encoding, m, f, a}} ->
        send(from, {:io_reply, ref, :ok})
        console(lines, [out, IO.chardata_to_string(apply(m, f, a))])

      {:io_request, from, ref, _other} ->
        send(from, {:io_reply, ref, {:error, :enotsup}})
        console(lines, out)

      {:written, to} ->
        send(to, {:written, IO.iodata_to_binary(out)})
    end
  end

  test "an expression's value is printed, and bindings last from line to line" do
    out = session("x = 20\nx * 2\n")
    assert out =~ "(1)> 20\n"
    assert out =~ "(2)> 40\n"
  end

  test "an unfinished expression is read over as many lines as it takes" do
    out =
      session(~S'''
      defmodule ShellTest.Greeter do
        def hi(name), do: "hi #{name}"
      end
      ShellTest.Greeter.hi(:you)
      [1,
       2]
      ''')

    assert out =~ "...(1)> ...(1)> {:module, ShellTest.Greeter"
    assert out =~ ~s("hi you")
    assert out =~ "[1, 2]"
  end

  test "an error is printed, the shell carries on, and the bindings are kept" do
    out = session("x = 1\nraise \"boom\"\nx + 1\n")
    assert out =~ "** (RuntimeError) boom"
    assert out =~ "(3)> 2\n"
  end

  test "a syntax error is reported and nothing of the line runs", %{tmp_dir: dir} do
    ran = Path.join(dir, "ran")
    out = session("File.write!(#{inspect(ran)}, \"\") )\n:after\n")
    assert out =~ "** (SyntaxError) shell:1:"
    assert out =~ ":after"
    refute File.exists?(ran)
  end

  test "an evaluation killed at its heap limit ends only itself" do
    out = session("x = 7\nEnum.reduce(1..10_000_000, [], &[&1 | &2])\nx\n", max_heap_words: 100_000)
    assert out =~ "** (EXIT) the evaluation ended: :killed"
    assert out =~ "(3)> 7\n"
  end

  test "an exit typed at the prompt ends only that line" do
    out = session("y = :kept\nexit(:bye)\ny\n")
    assert out =~ "** (exit) :bye"
    assert out =~ "(3)> :kept"
  end

  test "the helpers are there: cd, cat, grep, cp, mv", %{tmp_dir: dir} do
    File.write!(Path.join(dir, "app.log"), "ok\nerror one\nerror two\n")

    out =
      session("""
      cd #{inspect(dir)}
      cat("app.log") |> grep("error")
      cp "app.log", "copy.log"
      mv "copy.log", "moved.log"
      cat("moved.log") |> count()
      """)

    assert out =~ "error one\nerror two\n"
    assert out =~ "(5)> 3\n"
    assert File.exists?(Path.join(dir, "moved.log"))
  end

  test "help is at the prompt, and a wrong argument shows the usage, with no stack trace" do
    out = session("help()\nhelp(:cp)\nhead(ls(), -1)\n")

    assert out =~ "Files\n  cd "
    assert out =~ "> cp(src, dst)\n\nCopy a file\n"
    assert out =~ "** (Redoubt.Commandlet.UsageError) head: count must be an integer >= 0, got -1\n"
    assert out =~ ~S"usage: head(lines, count \\ 10)" <> "\n"
    refute out =~ "commandlet.ex"
  end

  test "a line holding bytes that are not UTF-8 is an error, and the shell reads the next" do
    out = raw_session([<<"\"a", 0xFF, "b\"\n">>, "1 + 1\n"])

    assert out =~ "the line holds bytes that are not UTF-8: \"a<FF>b\""
    assert out =~ "(2)> 2\n"
  end

  test "the tokenizer's warnings come through the printer, not straight to the console" do
    out = session("?\\\e\n")

    assert out =~ "warning: found ?\\ followed by code point 0x1B"
    refute out =~ "\e"
  end

  test "an ended line's reason that cannot be inspected does not stop the shell" do
    out =
      session(~S'''
      defmodule Stuck do
        defstruct []
        defimpl Inspect do
          def inspect(_stuck, _opts), do: Process.sleep(:infinity)
        end
      end
      spawn_link(fn -> exit(%Stuck{}) end); Process.sleep(1000)
      1 + 1
      ''')

    assert out =~ "the evaluation ended: a reason that could not be shown in time"
    assert out =~ "> 2\n"
  end

  test "the compiler's errors come through the printer, and a bare command name gets a hint" do
    out = session("pwd\nnot_a_thing\nx = 1\n\"\\e[2J\" = x\n")

    assert out =~ ~s|error: undefined variable "pwd" (shell:1)\n  pwd is a command: call it as pwd()\n|
    assert out =~ ~s|error: undefined variable "not_a_thing" (shell:2)\n|
    refute out =~ "not_a_thing is a command"
    refute out =~ "cannot compile file"
    refute out =~ "\e"
  end

  test "table lays rows out as lines fitted to them" do
    out =
      session(~S"""
      table([["a", 1], ["bb", 22]])
      table([["name", "n"], ["x", :y], {"z", 3.5}], header: true, title: "t")
      table([["a"]], colour: true)
      """)

    assert out =~ "(1)> a  1\nbb 22\n"
    assert out =~ "(2)> ┌t───────┐\n│name n  │\n│x    y  │\n│z    3.5│\n└────────┘\n"

    assert out =~
             "** (Redoubt.Commandlet.UsageError) table: there is no option colour; the options are header, title"
  end

  test "text from a hostile file never reaches the console as a control sequence", %{tmp_dir: dir} do
    path = Path.join(dir, "evil.txt")
    File.write!(path, "title\e]0;pwned\a clip\e]52;c;aGk=\a csi\u009B6n \x7F\n")
    File.write!(Path.join(dir, "name\e[2J.txt"), "")

    out = session("cat(#{inspect(path)})\nls(#{inspect(dir)})\ncat(#{inspect(path <> "\e[31m")})\n")

    assert out =~ "title^[]0;pwned^G clip^[]52;c;aGk=^G csi<U+009B>6n ^?"
    assert out =~ "name^[[2J.txt"
    # An exception's message holds the name inspected, so its ESC is already escaped.
    assert out =~ ~S(evil.txt\e[31m": no such file)
    refute out =~ "\e"
    refute out =~ "\u009B"
  end

  test "exit() ends the shell from inside an expression, and keeps nothing after it" do
    out = session("x = 1\nif x == 1, do: exit()\n:never\n")
    assert out =~ "(2)> "
    refute out =~ ":never"
    refute out =~ "redoubt_shell_exit"
  end

  test "exit, or the end of the input, ends the shell" do
    out = session("exit\n:never\n")
    assert out =~ "(1)> "
    refute out =~ ":never"
    assert session("") =~ ~r/\(1\)> \z/
  end
end
