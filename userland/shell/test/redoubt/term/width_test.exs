defmodule Redoubt.Term.WidthTest do
  use ExUnit.Case, async: true

  alias Redoubt.Term.Width

  test "a narrow grapheme takes one column, a wide one two, combining marks none of their own" do
    assert Width.columns("abc") == 3
    assert Width.columns("世界") == 4
    assert Width.columns("e\u0301") == 1
    assert Width.columns("가Ａ🙂") == 6
    assert Width.columns("") == 0
  end

  test "a presentation selector after the first code point makes a grapheme wide, as OTP decides" do
    assert Width.grapheme("❤\uFE0F") == 2
    assert Width.grapheme("#\uFE0E") == 2
  end

  test "a byte that is not UTF-8 takes one column" do
    assert Width.columns(<<"a", 0xFF, "b">>) == 3
  end

  test "it is OTP's judgement, grapheme by grapheme" do
    for g <- ["a", "é", "世", "🙂", "⌚", "Ａ", "ｱ", "e\u0301", "👍🏽"] do
      expected = if :unicode_util.is_wide(String.to_charlist(g)), do: 2, else: 1
      assert Width.grapheme(g) == expected, g
    end
  end

  test "every code point before U+1100, measured without asking OTP, is narrow as OTP judges it" do
    for c <- 0..0x10FF do
      refute :unicode_util.is_wide([c]), Integer.to_string(c, 16)
      assert Width.grapheme(<<c::utf8>>) == 1
    end

    assert Width.grapheme("\u1100") == 2
  end

  # Whether two code points side by side are two graphemes is a rule on the pair (and, for
  # emoji sequences and flags, on a joiner or a flag beside them, which no code point of a run
  # is): so each code point of a run beside a letter, beside a mark, beside itself and beside a
  # joiner is its own grapheme, one column wide, as OTP segments and measures it.
  test "a run is code points each its own grapheme one column wide, wherever it stands" do
    runs = Enum.concat([0x20..0x7E, 0xA0..0xAC, 0xAE..0x2FF, 0x370..0x482, 0x48A..0x52F])

    for c <- runs do
      g = <<c::utf8>>
      assert Width.run(g) == {byte_size(g), 1}, Integer.to_string(c, 16)
      # A run of 100 code points, past the 64 its Erlang prefix takes, goes on by the VM's matcher:
      # the same set.
      assert Width.run(String.duplicate(g, 100) <> "\u0301") == {100 * byte_size(g), 100},
             Integer.to_string(c, 16)

      assert Width.grapheme(g) == 1

      for {other, alone} <- [{"a", true}, {"\u0301", false}, {"\u200D", false}, {g, true}, {"\u0E33", false}] do
        assert String.graphemes(other <> g) == [other, g], Integer.to_string(c, 16)
        if alone, do: assert(String.graphemes(g <> other) == [g, other], Integer.to_string(c, 16))
      end
    end

    # Every other code point ends a run, before it, wherever it stands.
    others =
      Enum.concat([0..0x10FF, 0x3000..0x3010, 0xFFF0..0x10010, 0x10FFF0..0x10FFFF]) -- Enum.to_list(runs)

    for c <- others do
      assert Width.run(<<c::utf8>> <> "a") == {0, 0}, Integer.to_string(c, 16)
      assert Width.run("a" <> <<c::utf8>>) == {1, 1}, Integer.to_string(c, 16)
      long = String.duplicate("a\u00e9", 50)
      assert Width.run(long <> <<c::utf8>> <> "a") == {150, 100}, Integer.to_string(c, 16)
    end

    assert Width.run("abéп\u0301x世") == {6, 4}
  end

  # The VM's matcher takes a run past its first 64 code points, in windows of 128 bytes and then
  # 512, each cut where a character starts. At every window's edge, and through runs longer than
  # the windows, what it measures is what the Erlang path measures a code point at a time: a code
  # point alone never reaches the matcher, so that path is the reference.
  test "past a run's first 64 code points the matcher measures what the Erlang path does, at every window's edge" do
    stops = ["\e", "\u0301", "\u00AD", "\u0483", "€", "🙂", "世", "\u0530"]
    goes = ["é", "я", "~"]

    for filler <- ["x", "é"], at <- edges(filler), special <- stops ++ goes do
      text = prefix(filler, at) <> special <> String.duplicate(filler, 600)
      assert Width.run(text) == by_code_points(text), "#{inspect(special)} at byte #{at} after #{filler}"
    end

    for filler <- ["x", "é", "я", "x\u00e9"], n <- [65, 80, 100, 600, 1500] do
      text = String.duplicate(filler, n)
      assert Width.run(text) == {byte_size(text), String.length(text)}, "#{n} of #{filler}"
    end
  end

  # The byte offsets about the windows' edges: where the Erlang prefix ends (64 code points), and
  # 128, 640 and 1152 bytes past it, five bytes each side.
  defp edges(filler) do
    start = 64 * byte_size(filler)
    for edge <- [start, start + 128, start + 640, start + 1152], at <- (edge - 5)..(edge + 5), do: at
  end

  # `at` bytes of `filler`, one byte of ASCII making up an odd count.
  defp prefix(filler, at) do
    String.duplicate(filler, div(at, byte_size(filler))) <> String.duplicate("x", rem(at, byte_size(filler)))
  end

  # Width.run a code point at a time, the Erlang path's own answer.
  defp by_code_points(text) do
    text
    |> String.codepoints()
    |> Enum.reduce_while({0, 0}, fn c, {bytes, points} ->
      if Width.run(c) == {byte_size(c), 1},
        do: {:cont, {bytes + byte_size(c), points + 1}},
        else: {:halt, {bytes, points}}
    end)
  end
end
