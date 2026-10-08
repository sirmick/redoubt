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
end
