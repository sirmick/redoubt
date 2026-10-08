defmodule Redoubt.Editor.ManagerTest do
  # The file manager, driven through its update as its screen would be, on both VMs. Each
  # verdict on files is read from the file system afterwards.
  use ExUnit.Case, async: false

  alias Redoubt.Editor.{Buffer, Manager}
  alias Redoubt.Test.Terminal

  @moduletag :tmp_dir

  defp key(key, mods \\ []), do: {:key, key, mods}
  defp f(n), do: key({:f, n})

  defp keys(state, keys) do
    Enum.reduce(keys, state, fn k, state ->
      {:cont, state} = Manager.update(k, state)
      state
    end)
  end

  defp typed(state, text), do: keys(state, text |> String.graphemes() |> Enum.map(&key/1))

  # The person answers later than the question's deaf moment.
  defp later(state), do: %{state | deaf_until: nil}

  defp start(dir) do
    {:cont, state} = Manager.update({:resize, 80, 20}, Manager.init(dir))
    state
  end

  # The selection moved to `name` in the current pane.
  defp select(state, name) do
    pane = Enum.at(state.panes, state.at)
    i = Enum.find_index(pane.entries, &(is_map(&1) and &1.name == name))
    assert i, "#{name} is listed"
    pane = %{pane | list: %{pane.list | selected: i}}
    %{state | panes: List.replace_at(state.panes, state.at, pane)}
  end

  defp seed(dir) do
    for d <- ["a/sub", "b"], do: File.mkdir_p!(Path.join(dir, d))
    File.write!(Path.join(dir, "a/one.txt"), "one\n")
    File.write!(Path.join(dir, "a/sub/two.txt"), "two\n")
    {Path.join(dir, "a"), Path.join(dir, "b")}
  end

  # Both panes: the left in `a`, the right in `b`, the left in front.
  defp panes(dir) do
    {a, b} = seed(dir)
    state = start(a)

    state =
      keys(state, [key(:tab)]) |> select_up() |> keys([key(:enter)]) |> select("b") |> keys([key(:enter)])

    assert Enum.at(state.panes, 1).dir == b
    keys(state, [key(:tab)])
  end

  defp select_up(state), do: put_selected(state, 0)

  defp put_selected(state, i) do
    pane = Enum.at(state.panes, state.at)
    %{state | panes: List.replace_at(state.panes, state.at, %{pane | list: %{pane.list | selected: i}})}
  end

  test "Enter goes into a directory and up through the pane's own path; Tab changes pane", %{tmp_dir: dir} do
    {a, _b} = seed(dir)
    state = start(a)
    assert hd(state.panes).entries |> hd() == :up
    state = state |> select("sub") |> keys([key(:enter)])
    assert hd(state.panes).dir == Path.join(a, "sub")
    state = state |> select_up() |> keys([key(:enter)])
    assert hd(state.panes).dir == a
    assert state |> keys([key(:tab)]) |> Map.get(:at) == 1
  end

  test "F5 copies and F6 moves to the other pane after asking; F7 makes a directory; F8 removes",
       %{tmp_dir: dir} do
    state = panes(dir)
    {a, b} = {Path.join(dir, "a"), Path.join(dir, "b")}

    state = state |> select("one.txt") |> keys([f(5)])
    assert [%{id: {:copy, ^a, "one.txt", ^b}}] = state.dialogs.stack
    state = state |> later() |> keys([key(:enter)])
    assert File.read!(Path.join(b, "one.txt")) == "one\n" and File.exists?(Path.join(a, "one.txt"))

    # Nothing is overwritten.
    state = state |> select("one.txt") |> keys([f(5)]) |> later() |> keys([key(:enter)])
    assert [%{id: :failed}] = state.dialogs.stack
    state = keys(state, [key(:enter)])

    state = state |> select("sub") |> keys([f(6)]) |> later() |> keys([key(:enter)])
    assert File.read!(Path.join(b, "sub/two.txt")) == "two\n" and not File.exists?(Path.join(a, "sub"))

    state = keys(state, [f(7)]) |> typed("new") |> keys([key(:enter)])
    assert File.dir?(Path.join(a, "new"))

    state = state |> select("one.txt") |> keys([f(8)]) |> later() |> keys([key(:enter)])
    refute File.exists?(Path.join(a, "one.txt"))

    # Esc, or No, does nothing.
    state = state |> select("new") |> keys([f(8)]) |> later() |> keys([key(:esc)])
    assert File.dir?(Path.join(a, "new")) and state.dialogs.stack == []
  end

  test "a pasted Enter cannot answer the question: the keys after it are dropped", %{tmp_dir: dir} do
    state = panes(dir) |> select("one.txt") |> keys([f(8)])
    state = keys(state, [key(:enter), key(:enter)])
    assert File.exists?(Path.join(dir, "a/one.txt"))
    assert [%{id: {:remove, _, "one.txt", _}}] = state.dialogs.stack
  end

  test "a paste longer than the deaf moment cannot answer either: the question stays deaf to it",
       %{tmp_dir: dir} do
    state = panes(dir) |> select("one.txt") |> keys([f(8)])

    # 5,000 keys, each with the rest still queued behind it, arriving over more than 300 ms;
    # then the Enter that ends the paste, with nothing behind it.
    for _ <- 1..5_000, do: send(self(), :queued_key)

    state =
      Enum.reduce(1..5_000, state, fn i, state ->
        receive do: (:queued_key -> :ok)
        if rem(i, 50) == 0, do: Process.sleep(4)
        {:cont, state} = Manager.update(key("x"), state)
        state
      end)

    state = keys(state, [key(:enter)])
    assert File.exists?(Path.join(dir, "a/one.txt"))
    assert [%{id: {:remove, _, "one.txt", _}}] = state.dialogs.stack
  end

  test "a name the listing refuses is shown, and nothing acts on it", %{tmp_dir: dir} do
    state = panes(dir)
    before = File.ls!(Path.join(dir, "a")) |> Enum.sort()

    # As a server's listing could give them: Files.list marks them refused.
    crafted =
      for name <- ["../b", "x/y", ".."],
          do: %{name: name, shown: name, ok: false, type: :refused, size: 0}

    pane = hd(state.panes)
    entries = pane.entries ++ crafted
    labels = Enum.map(entries, fn e -> if e == :up, do: "/..", else: "!" <> e.shown end)
    pane = %{pane | entries: entries, list: Redoubt.Screen.Widget.List.new(labels)}
    state = %{state | panes: [pane | tl(state.panes)]}

    for i <- (length(entries) - 3)..(length(entries) - 1),
        action <- [f(3), f(4), f(5), f(6), f(8), key(:enter)] do
      state = state |> put_selected(i) |> keys([action])
      assert state.dialogs.stack == [] and state.editor == nil
      assert state.message =~ "acted on by nothing"
    end

    assert File.ls!(Path.join(dir, "a")) |> Enum.sort() == before
    assert File.ls!(Path.join(dir, "b")) == []
  end

  test "F4 edits a file in the editor and Ctrl+Q comes back; F3 views it, read only", %{tmp_dir: dir} do
    state = panes(dir) |> select("one.txt") |> keys([f(4)])
    assert state.editor != nil
    state = state |> typed("X") |> keys([key("s", [:ctrl])])
    assert File.read!(Path.join(dir, "a/one.txt")) == "Xone\n"
    state = keys(state, [key("q", [:ctrl])])
    assert state.editor == nil

    state = state |> select("one.txt") |> keys([f(3)]) |> typed("Y")
    assert Buffer.text(hd(state.editor.docs).buffer) == "Xone\n"
    assert state.editor.message =~ "read only: viewing"
    assert File.read!(Path.join(dir, "a/one.txt")) == "Xone\n"
    assert state |> keys([key("q", [:ctrl])]) |> Map.get(:editor) == nil

    assert Manager.update(f(10), state |> keys([key("q", [:ctrl])])) == {:halt, :ok}
  end

  describe "on a screen" do
    unless Redoubt.Term.Buffer.available?() do
      @describetag skip: "the screen buffer is beamlet's natives, which the BEAM does not have"
    end

    test "fm draws both panes, a hostile name visibly; F4 edits and F10 leaves", %{tmp_dir: dir} do
      File.write!(Path.join(dir, "\e[31mred\e]0;t\a.txt"), "x\n")
      test = self()

      driver =
        spawn_link(fn ->
          Redoubt.Shell.Driver.run(
            input: :messages,
            output: fn bytes -> send(test, {:drawn, IO.iodata_to_binary(bytes)}) end,
            size: fn -> {80, 12} end,
            shell: {Redoubt.Shell, :start_link, [[banner: false]]}
          )
        end)

      t = screen(Terminal.new(80, 12))
      send(driver, {:beamlet_console, ~s|fm("#{dir}")\r|})
      t = screen(t, &(Terminal.text(&1) =~ "F5 copy"))
      assert Terminal.alternate?(t)
      assert Terminal.text(t) =~ "^[[31mred^[]0;t^G.txt"
      assert Terminal.text(t) =~ "/.."

      # Down to the file, F4 into the editor, Ctrl+Q back to the panes, F10 out.
      send(driver, {:beamlet_console, "\e[B"})
      t = screen(t)
      send(driver, {:beamlet_console, "\e[14~"})
      t = screen(t, &(Terminal.text(&1) =~ "^S save"))
      send(driver, {:beamlet_console, <<17>>})
      t = screen(t, &(Terminal.text(&1) =~ "F5 copy"))
      send(driver, {:beamlet_console, "\e[21~"})
      t = screen(t, &(not Terminal.alternate?(&1)))
      assert Terminal.text(t) =~ ":ok"

      send(driver, {:beamlet_console, "exit\r"})
      ref = Process.monitor(driver)
      assert_receive {:DOWN, ^ref, :process, ^driver, :normal}, 5_000
    end
  end

  # The terminal once the shell has drawn what it will, or (with `done`) once it shows that.
  defp screen(terminal, done \\ fn _terminal -> true end) do
    receive do
      {:drawn, bytes} -> terminal |> Terminal.feed(bytes) |> screen(done)
    after
      if(done.(terminal), do: 300, else: 10_000) -> terminal
    end
  end
end
