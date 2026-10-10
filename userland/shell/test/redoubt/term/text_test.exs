defmodule Redoubt.Term.TextTest do
  use ExUnit.Case, async: true

  alias Redoubt.Term.Text

  test "printable text, wide characters included, passes unchanged" do
    assert Text.visible("héllo, 世界 🙂 ~") == "héllo, 世界 🙂 ~"
  end

  test "C0 controls are drawn in caret notation" do
    assert Text.visible("\e[2J\a\r\0\b") == "^[[2J^G^M^@^H"
  end

  test "a clipboard write, a forged link and a status query are only characters" do
    assert Text.visible("\e]52;c;aGk=\a") == "^[]52;c;aGk=^G"
    assert Text.visible("\e]8;;http://x\e\\y\e]8;;\e\\") == "^[]8;;http://x^[\\y^[]8;;^[\\"
    assert Text.visible("\e[6n") == "^[[6n"
  end

  test "DEL and the C1 controls, which some terminals obey as ESC [, are visible" do
    assert Text.visible("\x7F") == "^?"
    assert Text.visible("\u009B2J") == "<U+009B>2J"
  end

  test "a bidirectional override or isolate is visible, so a name cannot be shown reordered" do
    assert Text.visible("invoice\u202Etxt.exe") == "invoice<U+202E>txt.exe"
    assert Text.visible("a\u2066b\u2069c") == "a<U+2066>b<U+2069>c"
    assert Text.visible("\u202Aa\u202C") == "<U+202A>a<U+202C>"
  end

  test "the control characters are the cells crate's, one rule for both" do
    for c <- Enum.concat([0..0x1F, 0x7F..0x9F, 0x202A..0x202E, 0x2066..0x2069]), do: assert(Text.control?(c))
    for c <- [0x20, ?a, 0xA0, 0x200E, 0x2029, 0x202F, 0x2065, 0x206A], do: refute(Text.control?(c))
  end

  test "a code point passes as itself exactly when it is not a control character, alone or in a run" do
    points = Enum.concat([0..0x3000, 0xD7F0..0xD7FF, 0xE000..0xE010, 0xFFF0..0x10010, 0x10FFF0..0x10FFFF])

    for c <- points do
      {text, hex} = {<<c::utf8>>, Integer.to_string(c, 16)}

      if Text.control?(c),
        do: refute(Text.visible(text) == text, hex),
        else: assert(Text.visible(text) == text, hex)

      # After a short run and after one of 100 code points, past the 64 a run's Erlang prefix
      # takes, so the VM's matchers find the code point; a tab's stop depends on where it is, and
      # has its own test.
      for run <- ["aé", String.duplicate("aé", 50)], c != ?\t do
        assert Text.visible(run <> text <> "z") == run <> Text.visible(text) <> "z", hex
      end
    end
  end

  # The VM's matchers take a printable run past its first 64 code points, in windows of 128 bytes
  # and then 512. At every window's edge, whatever falls there (a control of one byte, of two as
  # C2 8x, or of three as E2 80 AA; a character of two, three or four bytes cut by the edge; a byte
  # that is not UTF-8, or a sequence cut short; a combining mark; a tab), the text is drawn as the
  # Erlang path draws it a code point at a time: a code point alone never reaches the matchers, so
  # that path is the reference.
  test "past a run's first 64 code points the matchers draw what the Erlang path does, at every window's edge" do
    specials = [
      "\e",
      "\u0000",
      "\u007F",
      "\u0085",
      "\u009B",
      "\u202A",
      "\u2069",
      "\t",
      "é",
      "€",
      "🙂",
      "\u0301",
      <<0xFF>>,
      <<0xE2, 0x82>>,
      <<0xF0, 0x9F, 0x99>>
    ]

    for filler <- ["x", "é"], at <- edges(filler), special <- specials do
      text = prefix(filler, at) <> special <> String.duplicate(filler, 600)
      assert Text.visible(text, 3) == by_units(text, 3), "#{inspect(special)} at byte #{at} after #{filler}"
    end

    for filler <- ["x", "é", "я", "x\u00e9", "世"], n <- [65, 80, 100, 600, 1500] do
      text = String.duplicate(filler, n)
      assert Text.visible(text) == text, "#{n} of #{filler}"
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

  # `visible/2` a unit at a time (a code point, or a byte that is not one), the column carried:
  # the Erlang path's own answer.
  defp by_units(text, col) do
    {out, col} =
      text
      |> units()
      |> Enum.reduce({[], col}, fn unit, {out, col} ->
        {drawn, col} = Text.visible(unit, col)
        {[out, drawn], col}
      end)

    {IO.iodata_to_binary(out), col}
  end

  defp units(<<>>), do: []
  defp units(<<c::utf8, rest::binary>>), do: [<<c::utf8>> | units(rest)]
  defp units(<<b, rest::binary>>), do: [<<b>> | units(rest)]

  test "a tab after a run of text stops at the next multiple of eight code points, not bytes" do
    assert Text.visible("éé\tx") == "éé      x"
    assert Text.visible("世\tx") == "世       x"
    assert Text.visible("x", 7) == {"x", 8}
    assert Text.visible("привет\t", 1) == {"привет ", 8}
  end

  test "bytes that are not UTF-8 are shown in hex" do
    assert Text.visible(<<"a", 0xFF, 0xC3, "b">>) == "a<FF><C3>b"
  end

  test "chardata is made UTF-8, a byte that is not drawn as visible/1 draws it, controls kept" do
    assert Text.utf8(["a", <<0xFF, ?b>>, ?c, 0x202E, [<<0xE4, 0xB8>>]]) == "a<FF>bc\u202E<E4><B8>"
    assert Text.utf8(<<27, "[2J">>) == "\e[2J"
    assert Text.visible(<<0xFF>>) == Text.utf8(<<0xFF>>)
  end

  test "a tab moves to the next multiple of eight columns" do
    assert Text.visible("ab\tc") == "ab      c"
    assert Text.visible("\t") == "        "
  end

  test "whatever comes in, no control character goes out" do
    every_byte = for b <- 0..255, into: <<>>, do: <<b>>
    every_c1 = for c <- 0x80..0x9F, into: <<>>, do: <<c::utf8>>

    for input <- [every_byte, every_c1, every_byte <> every_c1] do
      out = Text.visible(input)
      assert String.valid?(out)
      assert out |> String.to_charlist() |> Enum.all?(&(&1 >= 0x20 and &1 != 0x7F and &1 not in 0x80..0x9F))
    end
  end
end
