defmodule Redoubt.TermTest do
  use ExUnit.Case, async: true

  alias Redoubt.Term
  alias Redoubt.Test.Terminal

  # Draws group's requests, in order, on a fresh terminal: the screen and the encoder after them.
  defp draw(requests, cols \\ 40, rows \\ 8) do
    Enum.reduce(requests, {Terminal.new(cols, rows), Term.new(cols, rows)}, fn request, {screen, term} ->
      {out, term} = Term.request(term, request)
      {Terminal.feed(screen, out), term}
    end)
  end

  # What edlin sends to begin a line with the prompt `> `.
  defp prompt, do: [:new_prompt, {:insert_chars, :unicode, "> "}]

  defp insert(text), do: {:insert_chars, :unicode, text}

  # What group sends as a line ends: the line drawn again, formatted, and a new prompt opened.
  defp ended(line),
    do: [{:redraw_prompt, ~c"> ", ~c"  ", {[], {Enum.reverse(~c"#{line}\n"), []}, []}}, :new_prompt]

  test "a prompt and typed text are drawn as typed, and the cursor follows" do
    {screen, term} = draw(prompt() ++ [insert("abc")])
    assert Terminal.lines(screen) == ["> abc"]
    assert Terminal.cursor(screen) == {0, 5}
    assert Term.line_open?(term)
    refute Term.line_empty?(term)
  end

  test "moving left and inserting draws the line again with the cursor where it was" do
    {screen, _term} = draw(prompt() ++ [insert("ac"), {:move_rel, -1}, insert("b")])
    assert Terminal.lines(screen) == ["> abc"]
    assert Terminal.cursor(screen) == {0, 4}
  end

  test "deleting before and after the cursor" do
    {screen, _term} = draw(prompt() ++ [insert("abcd"), {:move_rel, -2}, {:delete_chars, -1}])
    assert Terminal.lines(screen) == ["> acd"]
    assert Terminal.cursor(screen) == {0, 3}

    {screen, _term} =
      draw(prompt() ++ [insert("abcd"), {:move_rel, -2}, {:delete_chars, -1}, {:delete_chars, 1}])

    assert Terminal.lines(screen) == ["> ad"]
    assert Terminal.cursor(screen) == {0, 3}
  end

  test "a line longer than the terminal is wide wraps, and its cursor is placed on the right row" do
    {screen, _term} = draw(prompt() ++ [insert(String.duplicate("x", 12))], 10)
    assert Terminal.lines(screen) == ["> xxxxxxxx", "xxxx"]
    assert Terminal.cursor(screen) == {1, 4}

    {screen, _term} = draw(prompt() ++ [insert(String.duplicate("x", 12)), {:move_rel, -5}], 10)
    assert Terminal.cursor(screen) == {0, 9}

    {screen, _term} = draw(prompt() ++ [insert(String.duplicate("x", 12)), {:move_rel, -5}, insert("Y")], 10)
    assert Terminal.lines(screen) == ["> xxxxxxxY", "xxxxx"]
    assert Terminal.cursor(screen) == {1, 0}
  end

  test "a line exactly as wide as the terminal leaves the cursor at the next row's start" do
    {screen, _term} = draw(prompt() ++ [insert(String.duplicate("x", 8))], 10)
    assert Terminal.lines(screen) == ["> xxxxxxxx"]
    assert Terminal.cursor(screen) == {1, 0}

    {screen, _term} = draw(prompt() ++ [insert(String.duplicate("x", 8)), insert("y")], 10)
    assert Terminal.lines(screen) == ["> xxxxxxxx", "y"]
    assert Terminal.cursor(screen) == {1, 1}
  end

  test "a line ended is drawn again formatted, its result below it, and the next prompt below that" do
    {screen, term} =
      draw(prompt() ++ [insert("1 + 1")] ++ ended("1 + 1") ++ [{:put_chars, :unicode, "2\n"}] ++ prompt())

    assert Terminal.lines(screen) == ["> 1 + 1", "2", ">"]
    assert Terminal.cursor(screen) == {2, 2}
    assert Term.line_empty?(term)
  end

  test "text printed while a line is edited goes above it, and the line is drawn again below" do
    {screen, _term} =
      draw(prompt() ++ [insert("abc"), {:move_rel, -1}, {:put_chars_sync, :unicode, "note", :reply}])

    assert Terminal.lines(screen) == ["note", "> abc"]
    assert Terminal.cursor(screen) == {1, 4}
  end

  test "text printed with no line open leaves the cursor where it ends, and the prompt follows it" do
    {screen, _term} = draw([{:put_chars, :unicode, "a\nb"}] ++ prompt() ++ [insert("c")])
    assert Terminal.lines(screen) == ["a", "b> c"]
    assert Terminal.cursor(screen) == {1, 4}
  end

  test "every line the encoder ends has its carriage return, since the console translates nothing" do
    {screen, _term} = draw([{:put_chars, :unicode, "one\ntwo\n"}])
    assert Terminal.lines(screen) == ["one", "two"]
    assert Terminal.cursor(screen) == {2, 0}
  end

  # Printed text is drawn 4096 bytes at a time: a character the cut would fall in is not split
  # into bytes, and a tab after the cut stops where the whole line's would.
  test "a printed line longer than a piece is drawn as one line, whatever falls at the cut" do
    line = String.duplicate("a", 4095) <> "é" <> "\tx\n"
    {screen, _term} = draw([{:put_chars, :unicode, line}])
    # 4096 columns of letters, a tab to 4104 and the x: the last row starts at column 4080.
    assert List.last(Terminal.lines(screen)) ==
             String.duplicate("a", 15) <> "é" <> String.duplicate(" ", 8) <> "x"

    refute Terminal.text(screen) =~ "<"
    assert Terminal.cursor(screen) == {7, 0}
  end

  test "printed text drawn as its slices draws exactly what it would whole, and a line open is not sliced" do
    text = String.duplicate("ab\té", 30) <> "\n" <> String.duplicate("x", 150)
    whole = {:put_chars, :unicode, text}
    slices = Enum.to_list(Term.slices(Term.new(40, 8), whole, 100))
    assert length(slices) > 2
    assert Enum.all?(slices, fn {:put_chars, :unicode, s} -> String.valid?(s) and byte_size(s) <= 100 end)
    {by_slices, term} = draw(slices)
    {at_once, _term} = draw([whole])
    assert Terminal.lines(by_slices) == Terminal.lines(at_once)
    assert Terminal.cursor(by_slices) == Terminal.cursor(at_once)

    {_screen, open} = draw(prompt() ++ [insert("c")])
    assert Term.slices(open, whole, 100) == [whole]
    assert Term.slices(term, {:insert_chars, :unicode, text}, 100) == [{:insert_chars, :unicode, text}]
  end

  test "a control character in a prompt, in typed text or in printed text is drawn visibly" do
    {screen, _term} =
      draw([:new_prompt, insert("\e]0;x\a> "), insert("\u009Bq"), {:put_chars, :unicode, "\e[2J\u202Ez\n"}])

    assert Terminal.lines(screen) == ["^[[2J<U+202E>z", "^[]0;x^G> <U+009B>q"]
  end

  test "group's search prompt, and only it, is drawn in bold" do
    {screen, _term} = draw([:new_prompt, insert("\e[;1;4msearch:\e[0m "), insert("ab")])
    assert Terminal.lines(screen) == ["search: ab"]
    assert Terminal.bold?(screen, 0, 0)
    assert Terminal.bold?(screen, 0, 6)
    refute Terminal.bold?(screen, 0, 8)

    {screen, _term} = draw([:new_prompt, insert("\e[;1;4mfake:\e[0m ")])
    assert Terminal.lines(screen) == ["^[[;1;4mfake:^[[0m"]
    refute Terminal.bold?(screen, 0, 0)
  end

  test "text below the line is shown, paged, and taken away by the next key" do
    expand = {:put_expand, :unicode, "one\ntwo\nthree\nfour\nfive\nsix\nseven", 2}
    {screen, _term} = draw(prompt() ++ [insert("ab"), expand], 20, 6)
    assert Terminal.lines(screen) == ["> ab", "one", "two", "rows 1 to 2 of 7"]
    assert Terminal.cursor(screen) == {0, 4}

    {screen, _term} = draw(prompt() ++ [insert("ab"), expand, {:move_expand, 1}], 20, 6)
    assert Terminal.lines(screen) == ["> ab", "two", "three", "rows 2 to 3 of 7"]

    {screen, _term} = draw(prompt() ++ [insert("ab"), expand, insert("c")], 20, 6)
    assert Terminal.lines(screen) == ["> abc"]
    assert Terminal.cursor(screen) == {0, 5}
  end

  test "a wide character takes two columns" do
    {screen, _term} = draw(prompt() ++ [insert("世x")])
    assert Terminal.lines(screen) == ["> 世x"]
    assert Terminal.cursor(screen) == {0, 5}
  end

  # A line's lines each begin with a prompt edlin inserts as text, so a column counts from the
  # row's start, prompt included, as prim_tty counts it.
  test "a line of several lines is moved through by line, keeping the column" do
    {screen, _term} = draw(prompt() ++ [insert("abc\n  de"), {:move_line, -1}])
    assert Terminal.lines(screen) == ["> abc", "  de"]
    assert Terminal.cursor(screen) == {0, 4}

    {screen, _term} = draw(prompt() ++ [insert("abc\n  de"), {:move_combo, -1, -1, 0}, insert("X")])
    assert Terminal.lines(screen) == ["> aXbc", "  de"]
    assert Terminal.cursor(screen) == {0, 4}
  end

  test "the interrupt marks the line, opens a new one, and leaves no line open" do
    {screen, term} = draw(prompt() ++ [insert("abc"), {:move_rel, -1}])
    {out, term} = Term.interrupt(term)
    screen = Terminal.feed(screen, out)
    assert Terminal.lines(screen) == ["> abc^C"]
    assert Terminal.cursor(screen) == {1, 0}
    refute Term.line_open?(term)
    refute Term.line_empty?(term)
  end

  test "a line is empty with its prompt alone, and no line is empty before a prompt" do
    {_screen, term} = draw(prompt())
    assert Term.line_empty?(term)
    {_screen, term} = draw([])
    refute Term.line_empty?(term)
    {_screen, term} = draw(prompt() ++ [insert("x")])
    refute Term.line_empty?(term)
  end

  test "a resize lays the line out at the new width from its next redraw" do
    {screen, term} = draw(prompt() ++ [insert(String.duplicate("x", 12))], 20)
    assert Terminal.lines(screen) == ["> xxxxxxxxxxxx"]
    {out, _term} = term |> Term.resize(10, 8) |> Term.request(:redraw_prompt)
    screen = Terminal.feed(Terminal.new(10, 8), out)
    assert Terminal.lines(screen) == ["> xxxxxxxx", "xxxx"]
  end
end
