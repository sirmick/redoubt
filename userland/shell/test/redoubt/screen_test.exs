defmodule Redoubt.ScreenTest do
  # The whole stack: the driver, group and edlin, the shell, a screen's process and the screen
  # buffer's natives, on the model of a terminal. The shell's helpers include cd: run alone.
  use ExUnit.Case, async: false

  alias Redoubt.Test.Terminal

  unless Redoubt.Term.Buffer.available?() do
    @moduletag skip: "the screen buffer is beamlet's natives, which the BEAM does not have"
  end

  @cols 60
  @rows 14

  defp start(opts \\ []) do
    test = self()

    driver =
      spawn_link(fn ->
        Redoubt.Shell.Driver.run(
          [
            input: :messages,
            output: fn bytes -> send(test, {:drawn, IO.iodata_to_binary(bytes)}) end,
            size: fn -> {@cols, @rows} end,
            shell: {Redoubt.Shell, :start_link, [[banner: false]]}
          ] ++ opts
        )
      end)

    {driver, screen(Terminal.new(@cols, @rows))}
  end

  defp type(driver, bytes), do: send(driver, {:beamlet_console, bytes})

  # The terminal once the shell has drawn what it will, or (with `done`) once it shows that.
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

  test "pick shows the items in a box on the alternate screen, and Enter returns the one chosen" do
    {driver, t} = start()
    type(driver, ~S|pick(["red", "green", "blue"], title: "colour")| <> "\r")
    t = screen(t, &shows?(&1, "blue"))
    assert Terminal.alternate?(t)
    refute t.cursor
    assert shows?(t, "┌─ colour ─┐")
    assert shows?(t, "│ red")
    assert shows?(t, "Enter choose")
    {row, col} = find(t, "red")
    assert MapSet.member?(Terminal.style(t, row, col).modifiers, :reversed), "the first is selected"

    type(driver, "\e[B")
    t = screen(t)
    {row, col} = find(t, "green")
    assert MapSet.member?(Terminal.style(t, row, col).modifiers, :reversed)

    type(driver, "\r")
    t = screen(t, &(not Terminal.alternate?(&1)))
    refute Terminal.alternate?(t)
    assert t.cursor
    assert shows?(t, "(1)> pick(")
    assert shows?(t, ~s("green"))
    refute shows?(t, "│")
    ends(driver)
  end

  test "Esc leaves with nil" do
    {driver, t} = start(esc_timeout: 0)
    type(driver, ~S|pick([:a, :b])| <> "\r")
    t = screen(t, &shows?(&1, ":b"))
    type(driver, "\e")
    t = screen(t, &(not Terminal.alternate?(&1)))
    assert Terminal.lines(t) |> Enum.at(1) == "nil"
    ends(driver)
  end

  test "Ctrl+C ends the screen with nil, and the session goes on" do
    {driver, t} = start()
    type(driver, "kept = 1\r")
    type(driver, ~S|pick(["x", "y"])| <> "\r")
    t = screen(t, &shows?(&1, "│ y"))
    type(driver, "\x03")
    t = screen(t, &(not Terminal.alternate?(&1)))
    assert shows?(t, "nil")
    type(driver, "kept\r")
    t = screen(t)
    assert List.last(Enum.filter(Terminal.lines(t), &(&1 != ""))) =~ ~r/\(4\)>$|^1$/
    assert shows?(t, "\n1\n")
    ends(driver)
  end

  test "Ctrl+C kills a screen that traps exits, and the session goes on" do
    {driver, t} = start()
    type(driver, ~S|Redoubt.Screen.run(Redoubt.ScreenTest.Trapping, [])| <> "\r")
    t = screen(t, &shows?(&1, "trapping"))
    type(driver, "\x03")
    t = screen(t, &shows?(&1, ":killed"))
    refute Terminal.alternate?(t)
    ends(driver)
  end

  test "a shell that ends under a screen leaves the main screen shown" do
    {driver, t} = start()
    ref = Process.monitor(driver)
    type(driver, "Process.register(Process.group_leader(), :shell_group)\r")
    type(driver, ~S|pick(["x"])| <> "\r")
    t = screen(t, &shows?(&1, "│ x"))
    Process.exit(Process.whereis(:shell_group), :kill)
    assert_receive {:DOWN, ^ref, :process, ^driver, :normal}, 5_000
    t = screen(t)
    refute Terminal.alternate?(t)
  end

  test "an item holding control characters is drawn visibly, and drives nothing" do
    {driver, t} = start()
    type(driver, ~S|pick(["\e]52;c;aGk=\a", "\u202Eevil", "ok"])| <> "\r")
    t = screen(t, &shows?(&1, "│ ok"))
    assert shows?(t, "^[]52;c;aGk=^G")
    assert shows?(t, "<U+202E>evil")
    type(driver, "\e[B\r")
    t = screen(t, &(not Terminal.alternate?(&1)))
    assert shows?(t, ~S("\u202Eevil"))
    ends(driver)
  end

  test "output from another process while a screen is in front is drawn when it ends" do
    {driver, t} = start()

    # The probe is this module's function: beamlet cannot run a receive typed at the prompt.
    type(driver, ~S|Process.register(spawn(Redoubt.ScreenTest, :probe, []), :probe)| <> "\r")

    type(driver, ~S|pick(["one"])| <> "\r")
    t = screen(t, &shows?(&1, "│ one"))
    send(:probe, :go)
    t = screen(t)
    refute shows?(t, "held back"), "not drawn over the screen"
    type(driver, "\r")
    t = screen(t, &shows?(&1, "held back"))
    refute Terminal.alternate?(t)
    assert shows?(t, ~s("one"))
    ends(driver)
  end

  test "a long list scrolls to keep the selection in view; End and Home go to its ends" do
    {driver, t} = start()
    type(driver, ~S|pick(Enum.map(1..30, &"item #{&1}"))| <> "\r")
    t = screen(t, &shows?(&1, "item 1"))
    refute shows?(t, "item 30")
    type(driver, "\e[F")
    t = screen(t, &shows?(&1, "item 30"))
    refute shows?(t, "item 1 ")
    type(driver, "\e[H")
    t = screen(t, &shows?(&1, "item 1 "))
    type(driver, "\e[6~\r")
    t = screen(t, &(not Terminal.alternate?(&1)))
    assert shows?(t, ~s("item 10"))
    ends(driver)
  end

  defmodule Trapping do
    # A screen that traps exits, as a screen may.
    @behaviour Redoubt.Screen

    @impl true
    def init(_args) do
      Process.flag(:trap_exit, true)
      nil
    end

    @impl true
    def update(_event, state), do: {:cont, state}

    @impl true
    def view(_state, buffer, _size), do: Redoubt.Term.Buffer.put(buffer, 0, 0, "trapping")
  end

  @doc false
  # Writes once told to, from a process of its own: output that is not the screen's.
  def probe do
    receive do
      :go -> IO.puts("held back")
    end
  end

  # Where `text` begins on the screen.
  defp find(t, text) do
    t
    |> Terminal.lines()
    |> Enum.with_index()
    |> Enum.find_value(fn {line, row} ->
      case :binary.match(line, text) do
        {at, _} -> {row, String.length(binary_part(line, 0, at))}
        :nomatch -> nil
      end
    end)
  end
end
