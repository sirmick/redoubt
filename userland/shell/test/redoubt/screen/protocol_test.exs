defmodule Redoubt.Screen.ProtocolTest do
  # What the driver draws for a screen: only the buffer that screen registered when it opened,
  # only while it is in front, and only when it asks; every asker is answered. This test's process
  # is the screen, and its own buffers are the buffers.
  use ExUnit.Case, async: false

  alias Redoubt.Term.Buffer

  unless Buffer.available?() do
    @moduletag skip: "the screen buffer is beamlet's natives, which the BEAM does not have"
  end

  @cols 40
  @rows 6

  setup do
    test = self()

    driver =
      spawn(fn ->
        Redoubt.Shell.Driver.run(
          input: :messages,
          output: fn bytes -> send(test, {:drawn, IO.iodata_to_binary(bytes)}) end,
          size: fn -> {@cols, @rows} end,
          shell: {Redoubt.Shell, :start_link, [[banner: false]]}
        )
      end)

    # The driver ends as a session's does, at `exit`, so it puts the logger's handler back.
    on_exit(fn ->
      ref = Process.monitor(driver)
      send(driver, {:beamlet_console, "exit\r"})

      receive do
        {:DOWN, ^ref, :process, ^driver, _reason} -> :ok
      after
        5_000 -> Process.exit(driver, :kill)
      end
    end)

    # The prompt drawn, then nothing more until the test draws.
    _ = collect("")
    %{driver: driver}
  end

  # Every byte the driver writes for a while.
  defp collect(acc) do
    receive do
      {:drawn, bytes} -> collect(acc <> bytes)
    after
      500 -> acc
    end
  end

  defp open(driver) do
    send(driver, {:redoubt_screen, :open, self(), :interrupt})
    assert_receive {:redoubt_screen, :opened, @cols, @rows}, 5_000
    _ = collect("")
  end

  defp buffer(text) do
    b = Buffer.new(@cols, @rows)
    Buffer.put(b, 0, 0, text)
    b
  end

  # Asks the driver to draw `b`, as `Redoubt.Screen.show/2` does: answered whether drawn or not.
  defp ask(driver, b) do
    send(driver, {:redoubt_screen, :draw, self(), b})
    assert_receive {:redoubt_screen, :drawn, ^driver}, 5_000
    collect("")
  end

  test "only the buffer registered when the screen opened is drawn: a foreign one, or a second, is not",
       %{driver: driver} do
    open(driver)
    mine = buffer("mine")
    Redoubt.Screen.register(driver, mine)
    foreign = buffer("XXXXXXXX")
    refute ask(driver, foreign) =~ "XXXXXXXX"
    Redoubt.Screen.register(driver, foreign)
    refute ask(driver, foreign) =~ "XXXXXXXX"
    assert ask(driver, mine) =~ "mine"
    send(driver, {:redoubt_screen, :close, self()})
  end

  test "a screen that has closed, or was never in front, is answered and nothing of it drawn",
       %{driver: driver} do
    open(driver)
    b = buffer("before")
    Redoubt.Screen.register(driver, b)
    assert ask(driver, b) =~ "before"
    send(driver, {:redoubt_screen, :close, self()})
    _ = collect("")
    Buffer.put(b, 0, 1, "XXXXXXXX")
    refute ask(driver, b) =~ "XXXXXXXX"

    # Another process, in front of nothing, with a buffer of its own.
    test = self()

    spawn(fn ->
      other = buffer("XXXXXXXX")
      send(driver, {:redoubt_screen, :draw, self(), other})
      receive do: ({:redoubt_screen, :drawn, ^driver} -> send(test, :answered))
    end)

    assert_receive :answered, 5_000
    refute collect("") =~ "XXXXXXXX"
  end

  test "a screen that registers a term that is no buffer is ended at its first draw", %{driver: driver} do
    {screen, ref} =
      spawn_monitor(fn ->
        send(driver, {:redoubt_screen, :open, self(), :interrupt})
        receive do: ({:redoubt_screen, :opened, _, _} -> :ok)
        not_a_buffer = make_ref()
        Redoubt.Screen.register(driver, not_a_buffer)
        Redoubt.Screen.show(driver, not_a_buffer)
      end)

    assert_receive {:DOWN, ^ref, :process, ^screen, :refused_frame}, 5_000
  end
end
