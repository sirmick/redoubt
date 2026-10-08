defmodule Redoubt.Editor.BufferTest do
  use ExUnit.Case, async: true

  alias Redoubt.Editor.Buffer

  defp typed(b, text), do: text |> String.graphemes() |> Enum.reduce(b, &Buffer.insert(&2, &1))

  test "text read and given back unedited is the same bytes: final newline, none, \\r, empty" do
    for text <- ["", "\n", "a", "a\n", "a\r\nb\r\n", "one\n\ntwo", "世界\n\t\e[31m\n"] do
      assert text |> Buffer.new() |> Buffer.text() == text
    end
  end

  test "the cursor moves by grapheme and line, keeps its column going up and down, and stays inside" do
    b = Buffer.new("long line\nab\nanother long line")
    b = Buffer.move(b, {:to, 0, 7})
    b = Buffer.move(b, :down)
    assert Buffer.cursor(b) == {1, 2}
    b = Buffer.move(b, :down)
    assert Buffer.cursor(b) == {2, 7}, "the column the move started from"
    assert b |> Buffer.move(:right) |> Buffer.cursor() == {2, 8}
    assert b |> Buffer.move(:end) |> Buffer.move(:right) |> Buffer.cursor() == {2, 17}
    assert b |> Buffer.move(:top) |> Buffer.move(:left) |> Buffer.cursor() == {0, 0}
    assert b |> Buffer.move(:home) |> Buffer.move(:left) |> Buffer.cursor() == {1, 2}
    assert b |> Buffer.move({:page, -10}) |> Buffer.cursor() == {0, 7}
    assert b |> Buffer.move(:bottom) |> Buffer.cursor() == {2, 17}
    assert Buffer.new("é世x") |> Buffer.move(:right) |> Buffer.move(:right) |> Buffer.cursor() == {0, 2}
  end

  test "words: to the start of the next, and of this or the last" do
    b = Buffer.new("foo bar_1, baz\nnext")
    assert b |> Buffer.move(:word_right) |> Buffer.cursor() == {0, 4}
    assert b |> Buffer.move({:to, 0, 6}) |> Buffer.move(:word_left) |> Buffer.cursor() == {0, 4}
    assert b |> Buffer.move(:end) |> Buffer.move(:word_right) |> Buffer.cursor() == {1, 0}
  end

  test "typing, a newline, backspace and delete edit at the cursor, joining lines at the edges" do
    b = Buffer.new("ab\ncd") |> Buffer.move({:to, 0, 1}) |> typed("X")
    assert Buffer.text(b) == "aXb\ncd"
    b = Buffer.insert(b, "\n")
    assert Buffer.text(b) == "aX\nb\ncd" and Buffer.cursor(b) == {1, 0} and Buffer.size(b) == 3
    b = Buffer.backspace(b)
    assert Buffer.text(b) == "aXb\ncd" and Buffer.cursor(b) == {0, 2}
    b = b |> Buffer.move(:end) |> Buffer.delete()
    assert Buffer.text(b) == "aXbcd" and Buffer.size(b) == 1
    assert b |> Buffer.move(:top) |> Buffer.backspace() |> Buffer.text() == "aXbcd"
    assert b |> Buffer.move(:end) |> Buffer.delete() |> Buffer.text() == "aXbcd"
  end

  test "a paste of several lines is put in place, the cursor after it" do
    b = Buffer.new("start end") |> Buffer.move({:to, 0, 6}) |> Buffer.insert("one\ntwo\nthree ")
    assert Buffer.text(b) == "start one\ntwo\nthree end"
    assert Buffer.cursor(b) == {2, 6}
    assert Buffer.slice(b, 1, 5) == ["two", "three end"]
    assert Buffer.slice(b, 0, 2) == ["start one", "two"]
  end

  test "a selection is copied as text, and replaced by what is typed or deleted" do
    b = Buffer.new("alpha\nbeta\ngamma") |> Buffer.move({:to, 0, 2})
    b = b |> Buffer.move(:down, select: true) |> Buffer.move(:right, select: true)
    assert Buffer.selection(b) == {{0, 2}, {1, 3}}
    assert Buffer.selected(b) == "pha\nbet"
    assert b |> Buffer.insert("Z") |> Buffer.text() == "alZa\ngamma"
    assert b |> Buffer.backspace() |> Buffer.text() == "ala\ngamma"
    assert b |> Buffer.move(:left) |> Buffer.selection() == nil
    assert Buffer.new("x\ny") |> Buffer.select_all() |> Buffer.selected() == "x\ny"
  end

  test "undo and redo go a step at a time, typed runs are one step, and modified? follows" do
    b = Buffer.new("x")
    refute Buffer.modified?(b)
    b = b |> Buffer.move(:end) |> typed("abc")
    assert Buffer.text(b) == "xabc" and Buffer.modified?(b)
    b = b |> Buffer.insert("\n") |> typed("de")
    assert Buffer.text(b) == "xabc\nde"

    b = Buffer.undo(b)
    assert Buffer.text(b) == "xabc\n"
    b = b |> Buffer.undo() |> Buffer.undo()
    assert Buffer.text(b) == "x"
    refute Buffer.modified?(b), "back where it was made"
    assert b |> Buffer.undo() |> Buffer.text() == "x"

    b = Buffer.redo(b)
    assert Buffer.text(b) == "xabc"
    saved = Buffer.mark_saved(b)
    refute Buffer.modified?(saved)
    assert saved |> Buffer.undo() |> Buffer.modified?()
    assert saved |> Buffer.undo() |> Buffer.redo() |> Buffer.modified?() == false

    # An edit after an undo is never taken for the saved text, nor can it be redone over.
    other = saved |> Buffer.undo() |> typed("q")
    assert Buffer.text(other) == "xq" and Buffer.modified?(other)
    assert Buffer.redo(other) == other
  end

  # The heap a process holds after a full collection, in words, with what `fun` gives kept live.
  defp held(fun) do
    Task.await(
      Task.async(fn ->
        kept = fun.()
        :erlang.garbage_collect()
        {:total_heap_size, words} = Process.info(self(), :total_heap_size)
        {words, kept}
      end),
      :infinity
    )
  end

  test "undo after edits across a 2 MiB file of short lines stays within its bound, newest first" do
    text = String.duplicate("short line #00\n", div(2 * 1024 * 1024, 15))

    across = fn b ->
      b |> Buffer.move(:top) |> Buffer.insert("a") |> Buffer.move(:bottom) |> Buffer.insert("b")
    end

    {opened, b0} = held(fn -> Buffer.new(text) end)
    {edited, b} = held(fn -> Enum.reduce(1..250, Buffer.new(text), fn _, b -> across.(b) end) end)

    # What the steps hold, by their own count: never more than the bound past the newest step's.
    [_newest | older] = b.undo
    assert Enum.sum(Enum.map(older, & &1.relinked)) <= Buffer.undo_lines()
    assert length(b.undo) < 500 and length(b.undo) > 2

    # The newest steps are the ones kept: undo goes back through the last edits, in order.
    t = Buffer.text(b)
    assert b |> Buffer.undo() |> Buffer.text() == binary_part(t, 0, byte_size(t) - 1)
    assert b |> Buffer.undo() |> Buffer.undo() |> Buffer.text() == binary_part(t, 1, byte_size(t) - 2)

    # On the BEAM, whose heap sizes are exact: the buffer, undo and all, in half the heap a screen
    # may grow to (16M words, Redoubt.Screen), where 500 unbounded steps would hold some 170M.
    unless Redoubt.Term.Buffer.available?() do
      assert edited <= div(16 * 1024 * 1024, 2),
             "#{edited} words, #{opened} on opening #{Buffer.size(b0)} lines"
    end
  end

  test "undo of edits on one long line stays within its bound: each step's copy of the line counts" do
    line = String.duplicate("x", 1024 * 1024)
    edit = fn b -> b |> Buffer.insert("a") |> Buffer.move(:left) end

    {_words, {b, binaries}} =
      held(fn -> Enum.reduce(1..500, Buffer.new(line), fn _, b -> edit.(b) end) |> with_binaries() end)

    [newest | older] = b.undo
    assert newest.relinked >= div(byte_size(line), 16)
    assert Enum.sum(Enum.map(older, & &1.relinked)) <= Buffer.undo_lines()
    assert length(b.undo) < 100
    assert b |> Buffer.undo() |> Buffer.text() |> byte_size() == byte_size(line) + 499

    # On the BEAM: the copies of the line the steps keep, off the heap, come to some 16 MiB, where
    # 500 would be 500 MiB.
    unless Redoubt.Term.Buffer.available?() do
      assert binaries <= 32 * 1024 * 1024, "#{binaries} bytes of binaries held"
    end
  end

  # The buffer, and on the BEAM the bytes of the binaries the process holds off its heap, each
  # once (beamlet does not list them: 0).
  defp with_binaries(b) do
    :erlang.garbage_collect()

    if Redoubt.Term.Buffer.available?() do
      {b, 0}
    else
      {:binary, binaries} = Process.info(self(), :binary)
      {b, binaries |> Enum.uniq_by(&elem(&1, 0)) |> Enum.map(&elem(&1, 1)) |> Enum.sum()}
    end
  end

  test "find selects the next match after the cursor, around the end, by string or Regex" do
    b = Buffer.new("one two\nthree two\nfour")
    {:ok, b1} = Buffer.find(b, "two")
    assert Buffer.selection(b1) == {{0, 4}, {0, 7}}
    {:ok, b2} = Buffer.find(b1, "two")
    assert Buffer.selection(b2) == {{1, 6}, {1, 9}}
    {:ok, b3} = Buffer.find(b2, "two")
    assert Buffer.selection(b3) == {{0, 4}, {0, 7}}, "around to the start"
    {:ok, r} = Buffer.find(b, ~r/f\w+/)
    assert Buffer.selected(r) == "four"
    assert Buffer.find(b, "absent") == :none
    assert Buffer.find(b, ~r/x*/) == :none, "an empty match is no match"
    {:ok, w} = Buffer.find(Buffer.new("aé世b"), "世")
    assert Buffer.selection(w) == {{0, 2}, {0, 3}}, "columns in graphemes"
  end

  test "replace: every match as one step, or the next match after the one selected" do
    b = Buffer.new("cat hat\nbat\nnone")
    {all, 2} = Buffer.replace_all(b, ~r/(\w)at/, "\\1og")
    assert Buffer.text(all) == "cog hog\nbog\nnone"
    assert all |> Buffer.undo() |> Buffer.text() == "cat hat\nbat\nnone"
    assert Buffer.replace_all(b, "zzz", "y") == {b, 0}

    {:ok, found} = Buffer.find(b, "at")
    {:ok, next} = Buffer.replace_next(found, "at", "AT")
    assert Buffer.text(next) == "cAT hat\nbat\nnone"
    assert Buffer.selected(next) == "at"
  end
end
