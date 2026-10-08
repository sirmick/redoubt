defmodule Redoubt.Screen.PagerTest do
  # The whole stack, as Redoubt.ScreenTest runs it: the driver, group and edlin, the shell, the
  # line's evaluator and the pager's screen, on the model of a terminal.
  use ExUnit.Case, async: false

  alias Redoubt.Test.Terminal

  unless Redoubt.Term.Buffer.available?() do
    @moduletag skip: "the screen buffer is beamlet's natives, which the BEAM does not have"
  end

  @moduletag :tmp_dir

  @cols 60
  @rows 14

  defp start(opts \\ []) do
    test = self()
    {shell_opts, opts} = Keyword.pop(opts, :shell, [])

    driver =
      spawn_link(fn ->
        Redoubt.Shell.Driver.run(
          [
            input: :messages,
            output: fn bytes -> send(test, {:drawn, IO.iodata_to_binary(bytes)}) end,
            size: fn -> {@cols, @rows} end,
            shell: {Redoubt.Shell, :start_link, [[banner: false] ++ shell_opts]}
          ] ++ opts
        )
      end)

    {driver, screen(Terminal.new(@cols, @rows))}
  end

  defp type(driver, bytes), do: send(driver, {:beamlet_console, bytes})

  defp screen(terminal, done \\ fn _terminal -> true end, wait \\ 10_000) do
    receive do
      {:drawn, bytes} -> terminal |> Terminal.feed(bytes) |> screen(done, wait)
    after
      if(done.(terminal), do: 300, else: wait) -> terminal
    end
  end

  defp shows?(terminal, text), do: Terminal.text(terminal) =~ text

  defp ends(driver) do
    type(driver, "exit\r")
    ref = Process.monitor(driver)
    assert_receive {:DOWN, ^ref, :process, ^driver, :normal}, 5_000
  end

  defp numbered(dir, n, extra \\ []) do
    path = Path.join(dir, "numbered.txt")
    File.write!(path, Enum.map_join(Enum.map(1..n, &"line #{&1}") ++ extra, "", &(&1 <> "\n")))
    path
  end

  test "lines that fit the screen are printed as they are, with no pager", %{tmp_dir: dir} do
    path = numbered(dir, 5)
    {driver, t} = start()
    type(driver, ~s|cat("#{path}")\r|)
    t = screen(t, &shows?(&1, "line 5"))
    refute Terminal.alternate?(t)
    assert shows?(t, "line 1\nline 2")
    ends(driver)
  end

  test "longer lines are paged: a page on, back, the end, and q leaves with nothing printed", %{tmp_dir: dir} do
    path = numbered(dir, 100)
    {driver, t} = start()
    type(driver, ~s|cat("#{path}")\r|)
    t = screen(t, &shows?(&1, "line 13"))
    assert Terminal.alternate?(t)
    assert List.first(Terminal.lines(t)) == "line 1"
    assert List.last(Terminal.lines(t)) =~ "lines 1-13 of 100 · Space next"
    refute shows?(t, "line 14")

    type(driver, " ")
    t = screen(t, &shows?(&1, "line 13\n"))
    assert List.first(Terminal.lines(t)) == "line 13"

    type(driver, "b")
    t = screen(t)
    assert List.first(Terminal.lines(t)) == "line 1"

    type(driver, "G")
    t = screen(t, &shows?(&1, "line 100"))
    assert shows?(t, "lines 88-100 of 100 (END)")

    type(driver, "q")
    t = screen(t, &(not Terminal.alternate?(&1)))
    refute Terminal.alternate?(t)
    assert List.last(Enum.reject(Terminal.lines(t), &(&1 == ""))) =~ ~r/\(2\)> ?$/
    refute shows?(t, "line 1\n")
    ends(driver)
  end

  test "a search finds forward and back, and says when there is nothing", %{tmp_dir: dir} do
    path = numbered(dir, 300)
    {driver, t} = start()
    type(driver, ~s|cat("#{path}")\r|)
    t = screen(t, &shows?(&1, "line 13"))

    type(driver, "/line 25")
    t = screen(t, &shows?(&1, "/line 25"))
    type(driver, "\r")
    t = screen(t, &(List.first(Terminal.lines(&1)) == "line 25"))
    assert List.first(Terminal.lines(t)) == "line 25"
    assert MapSet.member?(Terminal.style(t, 0, 0).modifiers, :reversed), "the match is drawn reversed"

    type(driver, "n")
    t = screen(t, &(List.first(Terminal.lines(&1)) == "line 250"))
    assert List.first(Terminal.lines(t)) == "line 250"

    type(driver, "N")
    t = screen(t, &(List.first(Terminal.lines(&1)) == "line 25"))
    assert List.first(Terminal.lines(t)) == "line 25"

    type(driver, "/absent\r")
    t = screen(t, &shows?(&1, "not found: absent"))
    assert shows?(t, "not found: absent")
    type(driver, "q")
    _t = screen(t, &(not Terminal.alternate?(&1)))
    ends(driver)
  end

  test "a hostile line in the pager is drawn visibly", %{tmp_dir: dir} do
    path = numbered(dir, 40, ["\e]52;c;aGk=\a", "\u202Eevil"])
    {driver, t} = start()
    type(driver, ~s|cat("#{path}")\r|)
    t = screen(t, &shows?(&1, "line 13"))
    type(driver, "G")
    t = screen(t, &shows?(&1, "evil"))
    assert shows?(t, "^[]52;c;aGk=^G")
    assert shows?(t, "<U+202E>evil")
    type(driver, "q")
    _t = screen(t, &(not Terminal.alternate?(&1)))
    ends(driver)
  end

  test "the pager reads only what it shows, and leaving it stops the reading" do
    Process.register(self(), :pager_test)

    {driver, t} = start()

    type(
      driver,
      ~S'Stream.resource(fn -> 0 end, fn n -> send(:pager_test, {:read, n}); {["row #{n}"], n + 1} end, fn _ -> send(:pager_test, :closed) end) |> Redoubt.Util.Lines.new()' <>
        "\r"
    )

    t = screen(t, &shows?(&1, "row 12"))
    assert Terminal.alternate?(t)
    refute_received {:read, 1000}
    refute_received :closed

    type(driver, "q")
    _t = screen(t, &(not Terminal.alternate?(&1)))
    assert_receive :closed, 5_000
    ends(driver)
  end

  test "help is drawn in the pager with its headings bold; filtered, it is plain" do
    {driver, t} = start()
    type(driver, "help()\r")
    t = screen(t, &shows?(&1, "Commands, by area"))
    assert Terminal.alternate?(t)
    row = Enum.find_index(Terminal.lines(t), &(&1 == "Files"))
    assert row, "an area heading is on the first page"
    assert Terminal.bold?(t, row, 0)
    refute Terminal.bold?(t, 0, 0), "the index's first line is not a heading"
    type(driver, "q")
    t = screen(t, &(not Terminal.alternate?(&1)))

    type(driver, ~S'help() |> grep("Files")' <> "\r")
    t = screen(t, &shows?(&1, "(3)>"))
    refute Terminal.alternate?(t)
    row = Enum.find_index(Terminal.lines(t), &(&1 == "Files"))
    refute Terminal.bold?(t, row, 0)
    ends(driver)
  end

  test "out prints every line without the pager", %{tmp_dir: dir} do
    path = numbered(dir, 40)
    {driver, t} = start()
    type(driver, ~s'cat("#{path}") |> out()\r')
    t = screen(t, &shows?(&1, ":ok"))
    refute Terminal.alternate?(t)
    assert shows?(t, "line 40\n:ok")
    ends(driver)
  end

  test "Ctrl+C ends the pager, and the session goes on", %{tmp_dir: dir} do
    path = numbered(dir, 100)
    {driver, t} = start()
    type(driver, ~s|cat("#{path}")\r|)
    t = screen(t, &shows?(&1, "line 13"))
    type(driver, "\x03")
    t = screen(t, &(not Terminal.alternate?(&1)))
    type(driver, "1 + 1\r")
    t = screen(t, &shows?(&1, "\n2\n"))
    assert shows?(t, "\n2\n")
    ends(driver)
  end

  test "lines paged past the line's heap limit end that line's evaluation, not the shell" do
    {driver, t} = start(shell: [max_heap_words: 2 * 1024 * 1024])

    type(
      driver,
      ~S'Stream.repeatedly(fn -> String.duplicate("x", 1000) end) |> Redoubt.Util.Lines.new()' <> "\r"
    )

    t = screen(t, &Terminal.alternate?/1)
    type(driver, "G")
    t = screen(t, &shows?(&1, "killed"), 60_000)
    refute Terminal.alternate?(t)
    assert shows?(t, "{:screen, :killed}")
    type(driver, "1 + 1\r")
    t = screen(t, &shows?(&1, "\n2\n"))
    assert shows?(t, "\n2\n")
    ends(driver)
  end
end
