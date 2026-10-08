defmodule Redoubt.Screen.Pager.DocTest do
  use ExUnit.Case, async: true

  alias Redoubt.Screen.Pager.Doc

  defp doc(lines, ended \\ true, style \\ nil), do: Doc.push(Doc.new(style), lines, ended)

  test "a line wider than the screen goes on in further rows, under its hanging indent" do
    assert Doc.wrap("abcdefgh", 3, 0) == ["abc", "def", "gh"]
    assert Doc.wrap("- one two three", 8, 2) == ["- one tw", "  o thre", "  e"]
    assert Doc.wrap("", 5, 0) == [""]
    # An indent of half the width or more would leave too little: none.
    assert Doc.wrap("abcdefgh", 4, 3) == ["abcd", "efgh"]
  end

  test "a wide grapheme the edge would cut starts the next row" do
    assert Doc.wrap("ab世c", 3, 0) == ["ab", "世c"]
    # Wider than the whole row: drawn alone, never looping.
    assert Doc.wrap("世世", 1, 0) == ["世", "世"]
  end

  test "a line is kept as it is shown: its control characters visible" do
    d = doc(["\e]52;c;aGk=\a", "\u202Eevil"])
    assert Doc.shown(d, 0) == {"^[]52;c;aGk=^G", false, 0}
    assert Doc.shown(d, 1) == {"<U+202E>evil", false, 0}
  end

  test "help's style bolds its headings and indents its list items, and is visible too" do
    d = doc(["# Title", "Examples", "cp(src, dst)", "  - an item", "plain text", "# \e[31mred"], true, :help)
    assert Doc.shown(d, 0) == {"Title", true, 0}
    assert Doc.shown(d, 1) == {"Examples", true, 0}
    assert Doc.shown(d, 2) == {"cp(src, dst)", true, 0}
    assert Doc.shown(d, 3) == {"  - an item", false, 4}
    assert Doc.shown(d, 4) == {"plain text", false, 0}
    assert Doc.shown(d, 5) == {"^[[31mred", true, 0}
  end

  test "rows are viewed from a place, across wrapped lines" do
    d = doc(["abcdef", "g", "hijk"])
    assert Doc.view(d, {0, 0}, 3, 3) == [{0, "abc", false}, {0, "def", false}, {1, "g", false}]

    assert Doc.view(d, {0, 1}, 3, 10) == [
             {0, "def", false},
             {1, "g", false},
             {2, "hij", false},
             {2, "k", false}
           ]
  end

  test "moving by rows stops at the first and the last row read" do
    d = doc(["abcdef", "g", "hijk"])
    assert Doc.down(d, {0, 0}, 1, 3) == {0, 1}
    assert Doc.down(d, {0, 0}, 3, 3) == {2, 0}
    assert Doc.down(d, {0, 0}, 99, 3) == {2, 1}
    assert Doc.up(d, {2, 1}, 2, 3) == {1, 0}
    assert Doc.up(d, {2, 1}, 99, 3) == {0, 0}
  end

  test "the last page puts the last row at the bottom, or starts at the top when all fits" do
    d = doc(Enum.map(1..10, &"line #{&1}"))
    assert Doc.last(d, 80, 4) == {6, 0}
    assert Doc.last(doc(["a", "b"]), 80, 4) == {0, 0}
    assert Doc.last(doc([]), 80, 4) == {0, 0}
  end

  test "find looks forward or back among the lines read, in what is shown" do
    d = doc(["alpha", "beta", "gamma", "beta again", "\e[1m"])
    assert Doc.find(d, "beta", 0, :next) == 1
    assert Doc.find(d, "beta", 1, :next) == 3
    assert Doc.find(d, "beta", 3, :next) == nil
    assert Doc.find(d, "beta", 3, :prev) == 1
    assert Doc.find(d, "beta", 0, :prev) == nil
    # A search is of the visible text: the escape cannot be found as itself.
    assert Doc.find(d, "\e", 0, :next) == nil
    assert Doc.find(d, "^[", 0, :next) == 4
  end

  test "lines pushed later follow the others, and the end is remembered" do
    d = Doc.push(Doc.new(), ["a"])
    refute d.ended
    d = Doc.push(d, ["b", "c"], true)
    assert Doc.count(d) == 3
    assert d.ended
    assert Doc.shown(d, 2) == {"c", false, 0}
  end
end
