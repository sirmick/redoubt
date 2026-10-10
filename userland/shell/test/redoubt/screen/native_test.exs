defmodule Redoubt.Screen.NativeTest do
  # A native program's screen through the whole stack: the driver, group and edlin, the shell, the
  # host and the screen buffer's natives, on the model of a terminal. The program is played by
  # this test, through `Program`, which stands where the pipeline's runner, feeder and reader stand.
  use ExUnit.Case, async: false

  alias Redoubt.Pipeline
  alias Redoubt.Term.Cells
  alias Redoubt.Test.Terminal

  unless Redoubt.Term.Buffer.available?() do
    @moduletag skip: "the screen buffer is beamlet's natives, which the BEAM does not have"
  end

  @cols 60
  @rows 14

  defmodule Program do
    @moduledoc false
    # A line types `Program.screen(opts)`; the test plays the program it starts.
    def screen(opts \\ []), do: Redoubt.Screen.Native.run("fake", [], [start: &start/1] ++ opts)

    defp start(host) do
      test = Process.whereis(:native_screen_test)
      send(host, {Pipeline, :stdin, self()})
      send(test, {:program, self()})
      loop(host, test, true)
    end

    defp loop(host, test, reads) do
      receive do
        {:write, bytes} ->
          if reads, do: send(host, {Pipeline, :written, byte_size(bytes)})
          send(test, {:stdin, bytes})
          loop(host, test, reads)

        {:reads, reads} ->
          loop(host, test, reads)

        {:out, bytes} ->
          send(host, {Pipeline, :stdout, self(), bytes})
          loop(host, test, reads)

        {Pipeline, :more} ->
          send(test, :more)
          loop(host, test, reads)

        {:end, result} ->
          result
      end
    end
  end

  setup do
    Process.register(self(), :native_screen_test)
    :ok
  end

  defp start do
    test = self()

    driver =
      spawn_link(fn ->
        Redoubt.Shell.Driver.run(
          input: :messages,
          output: fn bytes -> send(test, {:drawn, IO.iodata_to_binary(bytes)}) end,
          size: fn -> {@cols, @rows} end,
          shell: {Redoubt.Shell, :start_link, [[banner: false]]}
        )
      end)

    {driver, screen(Terminal.new(@cols, @rows))}
  end

  defp type(driver, bytes), do: send(driver, {:beamlet_console, bytes})

  defp screen(terminal, done \\ fn _terminal -> true end) do
    receive do
      {:drawn, bytes} -> terminal |> Terminal.feed(bytes) |> screen(done)
    after
      if(done.(terminal), do: 300, else: 10_000) -> terminal
    end
  end

  defp shows?(terminal, text), do: Terminal.text(terminal) =~ text

  defp ends(driver) do
    type(driver, "exit\r")
    ref = Process.monitor(driver)
    assert_receive {:DOWN, ^ref, :process, ^driver, :normal}, 5_000
  end

  # The screen started from a line, and the program's first event, its size.
  defp open(driver, t, opts \\ "") do
    type(driver, "Redoubt.Screen.NativeTest.Program.screen(#{opts})\r")
    assert_receive {:program, program}, 5_000
    assert_receive {:stdin, size}, 5_000
    assert size == Cells.record(<<1, @cols::little-16, @rows::little-16>>)
    {program, screen(t, &Terminal.alternate?/1)}
  end

  defp cell(x, y, symbol, fg \\ 0),
    do: <<x::little-16, y::little-16, byte_size(symbol), symbol::binary, fg, 0, 0, 0>> <> <<0::32, 0::16>>

  defp frame(cells, opts \\ []) do
    {w, h} = Keyword.get(opts, :size, {@cols, @rows})
    clear = if Keyword.get(opts, :clear, true), do: 1, else: 0
    Cells.record(<<1, clear, w::little-16, h::little-16, length(cells)::little-32>> <> Enum.join(cells))
  end

  defp out(program, bytes) do
    send(program, {:out, bytes})
    assert_receive :more, 5_000
  end

  test "a program's frames are drawn, keys reach it as events, and Ctrl+\\ ends it with nil" do
    {driver, t} = start()
    {program, t} = open(driver, t)
    assert Terminal.alternate?(t)

    out(program, frame([cell(0, 0, "h"), cell(1, 0, "i"), cell(3, 2, "界")]))
    t = screen(t, &shows?(&1, "hi"))
    assert Terminal.alternate?(t)
    assert shows?(t, "hi")
    assert shows?(t, "界")

    type(driver, "x\e[A")
    assert_receive {:stdin, x}, 5_000
    assert x == Cells.record(<<2, 0, 0, 1, ?x>>)
    assert_receive {:stdin, up}, 5_000
    assert up == Cells.record(<<2, 0, 5>>)

    # Ctrl+\ inside an escape sequence the key decoder was waiting on, and in a paste: the screen
    # ends, and neither the byte nor anything after it reaches the program.
    type(driver, "\e[1;5\x1cA")
    ref = Process.monitor(program)
    assert_receive {:DOWN, ^ref, :process, ^program, _reason}, 5_000
    t = screen(t, &(not Terminal.alternate?(&1)))
    refute Terminal.alternate?(t)
    assert shows?(t, "nil")
    refute_received {:stdin, _}
    ends(driver)
  end

  test "a paste holding the session's key ends the screen and reaches the program only up to it" do
    {driver, t} = start()
    {program, t} = open(driver, t)
    ref = Process.monitor(program)
    type(driver, "ab\x1ccd")
    assert_receive {:DOWN, ^ref, :process, ^program, _reason}, 5_000
    # The keys before it are sent the screen, which may end before it passes them on.
    got = stdin([])
    assert got in [[], [?a], [?a, ?b]]
    t = screen(t, &(not Terminal.alternate?(&1)))
    assert shows?(t, "nil")
    ends(driver)
  end

  defp stdin(acc) do
    receive do
      {:stdin, <<_length::little-32, 2, 0, 0, 1, key>>} -> stdin(acc ++ [key])
    after
      300 -> acc
    end
  end

  test "with ctrl_c: :key, Ctrl+C is the program's key and Ctrl+\\ alone ends it" do
    {driver, t} = start()
    {program, t} = open(driver, t, "ctrl_c: :key")
    type(driver, "\x03")
    assert_receive {:stdin, ctrl_c}, 5_000
    assert ctrl_c == Cells.record(<<2, 4, 0, 1, ?c>>)
    assert Terminal.alternate?(screen(t))
    ref = Process.monitor(program)
    type(driver, "\x1c")
    assert_receive {:DOWN, ^ref, :process, ^program, _reason}, 5_000
    t = screen(t, &(not Terminal.alternate?(&1)))
    assert shows?(t, "nil")
    ends(driver)
  end

  # Each refused: the program ended, the screen with it, and nothing of the record drawn.
  for {name, why, bytes} <- [
        {"a frame larger than its screen", :size, quote(do: frame([cell(70, 0, "X")], size: {80, 24}))},
        {"a frame smaller than its screen", :size, quote(do: frame([cell(0, 0, "X")], size: {20, 5}))},
        {"a cell outside its screen", :position, quote(do: frame([cell(60, 0, "X")]))},
        {"a symbol of many graphemes", :cell, quote(do: frame([cell(50, 0, String.duplicate("X", 32))]))},
        {"a wide symbol in the last column", :cell, quote(do: frame([cell(59, 0, "界")]))},
        {"a symbol holding ESC", :symbol, quote(do: frame([cell(0, 0, "\e]52;c;eA==\a")]))},
        {"a symbol holding the session's key", :symbol, quote(do: frame([cell(0, 0, "\x1c")]))},
        {"a record longer than any frame of its screen", :length, quote(do: <<0xFFFFFFFF::little-32>>)},
        {"text, not records", :length, quote(do: "\e]52;c;eA==\a\e]0;pwned\aXXXXXXXX\n")}
      ] do
    test "#{name} is refused #{why}, ending the program and drawing none of it" do
      {driver, t} = start()
      {program, t} = open(driver, t)
      out(program, frame([cell(0, 0, "o"), cell(1, 0, "k")]))
      t = screen(t, &shows?(&1, "ok"))
      ref = Process.monitor(program)
      send(program, {:out, unquote(bytes)})
      assert_receive {:DOWN, ^ref, :process, ^program, :killed}, 5_000

      raw = collect("")
      t = Terminal.feed(t, raw)
      t = screen(t, &(not Terminal.alternate?(&1)))
      refute Terminal.alternate?(t)
      assert shows?(t, "{:error, {:refused, :#{unquote(why)}}}")
      refute raw =~ "XXXXXXXX"
      refute raw =~ "\e]"
      refute raw =~ "\x1c"
      ends(driver)
    end
  end

  # What the session writes for a while: every byte.
  defp collect(acc) do
    receive do
      {:drawn, bytes} -> collect(acc <> bytes)
    after
      500 -> acc
    end
  end

  test "after a resize, frames of the old size are dropped, the new size's drawn, and a third refused" do
    {driver, t} = start()
    {program, t} = open(driver, t)
    send(driver, {:beamlet_console_resize, {40, 10}})
    assert_receive {:stdin, size}, 5_000
    assert size == Cells.record(<<1, 40::little-16, 10::little-16>>)

    out(program, frame([cell(0, 0, "o"), cell(1, 0, "l"), cell(2, 0, "d")]))
    t = screen(t)
    refute shows?(t, "old")

    out(program, frame([cell(0, 0, "n"), cell(1, 0, "e"), cell(2, 0, "w")], size: {40, 10}))
    t = screen(t, &shows?(&1, "new"))
    assert shows?(t, "new")

    ref = Process.monitor(program)
    send(program, {:out, frame([cell(0, 0, "o")])})
    assert_receive {:DOWN, ^ref, :process, ^program, :killed}, 5_000
    t = screen(t, &(not Terminal.alternate?(&1)))
    assert shows?(t, "{:error, {:refused, :size}}")
    ends(driver)
  end

  test "the program's output is asked for at most its bytes a second" do
    {driver, t} = start()
    {program, _t} = open(driver, t, "bytes_per_second: 60")
    # Half a record, then the rest, which spends the second's bytes: the next read waits for the
    # next second.
    record = frame([cell(0, 0, "a"), cell(1, 0, "b"), cell(2, 0, "c")])
    <<first::binary-size(32), rest::binary>> = record
    send(program, {:out, first})
    assert_receive :more, 5_000
    send(program, {:out, rest})
    refute_receive :more, 500
    assert_receive :more, 2_000
    type(driver, "\x1c")
    ends(driver)
  end

  test "keys the program has not read are dropped past their bound" do
    {driver, t} = start()
    {program, _t} = open(driver, t)
    send(program, {:reads, false})
    type(driver, String.duplicate("a", 10_000))
    sent = count_stdin(0)
    # Each key's record is 9 bytes; the size's was read before the program stopped reading.
    assert sent == div(64 * 1024, 9)
    type(driver, "\x1c")
    ends(driver)
  end

  defp count_stdin(n) do
    receive do
      {:stdin, _} -> count_stdin(n + 1)
    after
      1_000 -> n
    end
  end

  test "an ended program's value is the line's, and its standard error is drawn visibly after" do
    {driver, t} = start()
    {program, t} = open(driver, t)
    out(program, frame([cell(0, 0, "o"), cell(1, 0, "k")]))
    t = screen(t, &shows?(&1, "ok"))
    send(program, {:end, {:ok, %{endings: [{:exited, 3}], errors: {"\e]0;pwned\aoops\n", 5}}}})
    raw = collect("")
    t = screen(Terminal.feed(t, raw), &(not Terminal.alternate?(&1)))
    refute Terminal.alternate?(t)
    assert shows?(t, "^[]0;pwned^Goops")
    assert shows?(t, "(5 more bytes of standard error not kept)")
    assert shows?(t, "{:exited, 3}")
    refute raw =~ "\e]"
    ends(driver)
  end
end
