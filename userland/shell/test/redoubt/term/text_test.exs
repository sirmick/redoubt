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
