defmodule Redoubt.EditorTest do
  # The editor's keys and files, driven through its update as its screen would be, on both VMs;
  # its drawing on a model of the terminal, on beamlet alone.
  use ExUnit.Case, async: false

  alias Redoubt.Editor
  alias Redoubt.Editor.{Buffer, Files, View}
  alias Redoubt.Test.Terminal

  @moduletag :tmp_dir

  defp start(path) do
    {:ok, doc} = Editor.open(path)
    {:cont, state} = Editor.update({:resize, 60, 12}, Editor.init(doc))
    state
  end

  defp key(key, mods \\ []), do: {:key, key, mods}

  # Each key in turn, as typed: nothing else waiting behind it.
  defp keys(state, keys) do
    Enum.reduce(keys, state, fn k, state ->
      {:cont, state} = Editor.update(k, state)
      state
    end)
  end

  defp typed(state, text), do: keys(state, text |> String.graphemes() |> Enum.map(&key/1))
  defp doc(state), do: Enum.at(state.docs, state.at)
  defp text(state), do: Buffer.text(doc(state).buffer)

  test "a file opens as it is; a missing one is new; one not UTF-8 opens read only, bytes shown",
       %{tmp_dir: dir} do
    File.write!(Path.join(dir, "a.txt"), "one\ntwo\n")
    assert text(start(Path.join(dir, "a.txt"))) == "one\ntwo\n"

    new = start(Path.join(dir, "new.txt"))
    assert text(new) == "" and doc(new).digest == :absent and not doc(new).readonly

    File.write!(Path.join(dir, "bin"), <<"ok", 0xFF, 0xFE, "\n">>)
    bin = start(Path.join(dir, "bin"))
    assert doc(bin).readonly == :not_utf8 and text(bin) == "ok<FF><FE>\n"
    bin = typed(bin, "x")
    assert text(bin) == "ok<FF><FE>\n" and bin.message =~ "read only"
    assert keys(bin, [key("s", [:ctrl])]).message =~ "read only"
    assert File.read!(Path.join(dir, "bin")) == <<"ok", 0xFF, 0xFE, "\n">>

    File.write!(Path.join(dir, "big"), :binary.copy("x", Files.max_bytes() + 1))

    assert Editor.open(Path.join(dir, "big")) ==
             {:error, {:too_large, Files.max_bytes() + 1, Files.max_bytes()}}

    assert Editor.open(dir) == {:error, {:not_a_file, :directory}}
  end

  test "typing and Ctrl+S save the file; Ctrl+Q then closes it, and the editor with it", %{tmp_dir: dir} do
    path = Path.join(dir, "notes.txt")
    state = start(path) |> typed("hello") |> keys([key(:enter)]) |> typed("world")
    assert text(state) == "hello\nworld"
    refute File.exists?(path)

    state = keys(state, [key("s", [:ctrl])])
    assert File.read!(path) == "hello\nworld"
    assert state.message =~ "saved"
    refute Buffer.modified?(doc(state).buffer)
    assert Editor.update(key("q", [:ctrl]), state) == {:halt, :ok}
  end

  test "pasted keys cannot save or close: the question they raise drops the rest of the paste",
       %{tmp_dir: dir} do
    path = Path.join(dir, "notes.txt")
    File.write!(path, "kept\n")
    state = start(path) |> typed("edit ")

    # A paste: the keys after this one are already waiting, as a terminal sends them at once.
    send(self(), :more_keys_behind)
    {:cont, state} = Editor.update(key("s", [:ctrl]), state)
    assert_received :more_keys_behind
    assert File.read!(path) == "kept\n"
    assert [%{id: {:guard, :save}}] = state.dialogs.stack

    # The pasted Enter that would answer it arrives with it, and is dropped.
    state = keys(state, [key(:enter)])
    assert File.read!(path) == "kept\n"
    assert [%{id: {:guard, :save}}] = state.dialogs.stack

    # The person answers, later.
    state = keys(%{state | deaf_until: nil}, [key(:enter)])
    assert File.read!(path) == "edit kept\n"
    assert state.dialogs.stack == []

    send(self(), :more_keys_behind)
    {:cont, state} = Editor.update(key("q", [:ctrl]), state)
    assert_received :more_keys_behind
    assert [%{id: {:guard, :close}}] = state.dialogs.stack
  end

  test "a paste that ends in Ctrl+S, Ctrl+Q or Ctrl+O asks too, though nothing waits behind its last key",
       %{tmp_dir: dir} do
    path = Path.join(dir, "notes.txt")
    File.write!(path, "kept\n")

    for {guarded, command} <- [{"s", :save}, {"q", :close}, {"o", :open}] do
      state = start(path) |> typed("edit ")

      # The paste's text, with its last key waiting behind it; then that key, with nothing behind.
      send(self(), :last_key_behind)
      {:cont, state} = Editor.update(key("x"), state)
      assert_received :last_key_behind
      {:cont, state} = Editor.update(key(guarded, [:ctrl]), state)

      assert [%{id: {:guard, ^command}}] = state.dialogs.stack
      assert File.read!(path) == "kept\n"
    end

    # Typed later, by the person, the same key acts at once.
    state = start(path) |> typed("edit ")
    send(self(), :last_key_behind)
    {:cont, state} = Editor.update(key("x"), state)
    assert_received :last_key_behind
    state = %{state | burst_at: state.burst_at - 1_000}
    state = keys(state, [key("s", [:ctrl])])
    assert state.dialogs.stack == [] and File.read!(path) == "edit xkept\n"
  end

  test "closing with changes asks to save them; a file changed on disk is not overwritten unasked",
       %{tmp_dir: dir} do
    path = Path.join(dir, "notes.txt")
    File.write!(path, "base\n")
    state = start(path) |> typed("mine ")

    state = keys(state, [key("q", [:ctrl])])
    assert [%{id: :unsaved}] = state.dialogs.stack
    state = keys(state, [key(:esc)])
    assert state.dialogs.stack == [] and text(state) == "mine base\n"

    File.write!(path, "theirs\n")
    state = keys(state, [key("s", [:ctrl])])
    assert File.read!(path) == "theirs\n"
    assert [%{id: :overwrite}] = state.dialogs.stack
    state = keys(state, [key(:enter)])
    assert File.read!(path) == "mine base\n"

    state = typed(state, "!")
    state = keys(state, [key("q", [:ctrl])])
    assert Editor.update(key(:enter), state) == {:halt, :ok}
    assert File.read!(path) == "mine !base\n"
  end

  test "select, copy with Ctrl+C, cut and paste; undo and redo; find and replace through prompts",
       %{tmp_dir: dir} do
    path = Path.join(dir, "t.txt")
    File.write!(path, "cat hat\nbat\n")
    state = start(path)

    state =
      keys(state, [key(:right, [:shift]), key(:right, [:shift]), key(:right, [:shift]), key("c", [:ctrl])])

    assert state.clipboard == "cat"
    state = keys(state, [key(:end), key("v", [:ctrl])])
    assert text(state) == "cat hatcat\nbat\n"
    state = keys(state, [key("z", [:ctrl])])
    assert text(state) == "cat hat\nbat\n"
    state = keys(state, [key("y", [:ctrl])])
    assert text(state) == "cat hatcat\nbat\n"

    state = keys(state, [key("f", [:ctrl])]) |> typed("/b\\w+/") |> keys([key(:enter)])
    assert Buffer.selected(doc(state).buffer) == "bat"
    state = keys(state, [key("x", [:ctrl])])
    assert text(state) == "cat hatcat\n\n" and state.clipboard == "bat"

    # The prompt starts with the last pattern; Ctrl+U clears it.
    state = keys(state, [key("r", [:ctrl]), key("u", [:ctrl])]) |> typed("at") |> keys([key(:enter)])
    state = state |> typed("og") |> keys([key(:enter)])
    assert text(state) == "cog hogcog\n\n"
    assert state.message =~ "replaced on 1 line"

    state = keys(state, [key("l", [:ctrl])]) |> typed("2") |> keys([key(:enter)])
    assert Buffer.cursor(doc(state).buffer) == {1, 0}
  end

  test "Ctrl+O opens another file beside the first; Alt+. and Alt+, go between them", %{tmp_dir: dir} do
    File.write!(Path.join(dir, "a"), "A")
    File.write!(Path.join(dir, "b"), "B")
    state = start(Path.join(dir, "a"))
    state = keys(state, [key("o", [:ctrl])]) |> typed(Path.join(dir, "b")) |> keys([key(:enter)])
    assert length(state.docs) == 2 and text(state) == "B"
    assert state |> keys([key(".", [:alt])]) |> text() == "A"
    assert state |> keys([key(",", [:alt])]) |> text() == "A"
    state = keys(state, [key("q", [:ctrl])])
    assert length(state.docs) == 1 and text(state) == "A"
  end

  test "highlighting keeps start states down the file, and drops those below an edit", %{tmp_dir: dir} do
    path = Path.join(dir, "long.rs")
    File.write!(path, Enum.map_join(1..400, "\n", &"let x#{&1} = 1;"))
    state = start(path) |> keys([key(:end, [:ctrl])])
    assert doc(state).lang == Redoubt.Editor.Syntax.Rust and doc(state).top > 256
    assert doc(state).marks[2] == :code

    # A comment opened on the first line runs on through the file: what was kept below it goes.
    state = state |> keys([key(:home, [:ctrl])]) |> typed("/*")
    assert Map.keys(doc(state).marks) == [0]
    state = keys(state, [key(:end, [:ctrl])])
    assert doc(state).marks[2] == {:comment, 1}

    assert start(Path.join(dir, "notes.txt")) |> doc() |> Map.get(:lang) == nil
  end

  test "undo and redo drop the highlighting's states: their cursor is not where the text changed",
       %{tmp_dir: dir} do
    path = Path.join(dir, "long.rs")
    File.write!(path, Enum.map_join(1..400, "\n", &"let x#{&1} = 1;"))

    # Lines opening a comment pasted at the top; then to the end, where the comment runs on.
    state = %{start(path) | clipboard: "/*\nopen\n"} |> keys([key("v", [:ctrl]), key(:end, [:ctrl])])
    assert doc(state).marks[2] == {:comment, 1}

    # Undone, and down to the end again: the states are code.
    state = keys(state, [key("z", [:ctrl]), key(:end, [:ctrl])])
    assert doc(state).marks[2] == :code

    # Redone with the cursor at the end, far below the lines it puts back: the comment again.
    state = keys(state, [key("y", [:ctrl])])
    assert Buffer.cursor(doc(state).buffer) |> elem(0) > 256
    assert doc(state).marks[2] == {:comment, 1}
  end

  test "a line is drawn with tabs to the stop, control characters visible, clipped to the window" do
    assert View.runs("a\tb", 0, 20, nil, nil) == [{0, "a   b", :normal}]
    assert View.runs("\e[31mx", 0, 20, nil, nil) == [{0, "^[[31mx", :normal}]
    assert View.runs("ab", 0, 20, nil, 2) == [{0, "ab", :normal}, {2, " ", :cursor}]
    assert View.runs("abcd", 0, 20, 1..2, 0) == [{0, "a", :cursor}, {1, "bc", :selected}, {3, "d", :normal}]
    assert View.runs("abcdef", 2, 3, nil, nil) == [{0, "cde", :normal}]
    assert View.runs("a世b", 2, 5, nil, nil) == [{0, " b", :normal}], "half a wide character is a blank"
    assert View.runs("ab", 0, 20, 0..1, nil, true) == [{0, "ab ", :selected}]
    assert View.column("\tx世", 3) == 4 + 1 + 2
  end

  describe "on a screen" do
    unless Redoubt.Term.Buffer.available?() do
      @describetag skip: "the screen buffer is beamlet's natives, which the BEAM does not have"
    end

    test "ed draws the file, escapes and all, visibly; typing and Ctrl+S save; Ctrl+C copies; Ctrl+Q leaves",
         %{tmp_dir: dir} do
      path = Path.join(dir, "hostile.txt")
      File.write!(path, "title \e]0;pwned\a here\n")
      test = self()

      driver =
        spawn_link(fn ->
          Redoubt.Shell.Driver.run(
            input: :messages,
            output: fn bytes -> send(test, {:drawn, IO.iodata_to_binary(bytes)}) end,
            size: fn -> {60, 12} end,
            shell: {Redoubt.Shell, :start_link, [[banner: false]]}
          )
        end)

      t = screen(Terminal.new(60, 12))
      send(driver, {:beamlet_console, ~s|ed("#{path}")\r|})
      t = screen(t, &(Terminal.text(&1) =~ "^S save"))
      assert Terminal.alternate?(t)
      assert Terminal.text(t) =~ "title ^[]0;pwned^G here"
      assert Terminal.text(t) =~ "File" and Terminal.text(t) =~ "hostile.txt"

      # Typed, not pasted: Ctrl+S once the X is drawn, not on its heels.
      send(driver, {:beamlet_console, "X"})
      t = screen(t, &(Terminal.text(&1) =~ "Xtitle"))
      Process.sleep(300)
      send(driver, {:beamlet_console, <<19>>})
      t = screen(t, &(Terminal.text(&1) =~ "saved"))
      assert File.read!(path) == "Xtitle \e]0;pwned\a here\n"

      # Ctrl+C is the editor's copy, not the interrupt: the editor stays, and says what it copied.
      send(driver, {:beamlet_console, <<3>>})
      t = screen(t, &(Terminal.text(&1) =~ "nothing selected"))
      assert Terminal.alternate?(t)

      send(driver, {:beamlet_console, <<17>>})
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
