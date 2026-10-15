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

  test "an invisible format character is drawn, so two names cannot look the same" do
    assert Text.visible("rm re\u200Bport.txt") == "rm re<U+200B>port.txt"
    assert Text.visible("re\u00ADport") == "re<U+00AD>port"
    assert Text.visible("a\u200Cb\u2060c\u2064d") == "a<U+200C>b<U+2060>c<U+2064>d"
    assert Text.visible("\uFEFFa\u2028b\u2029") == "<U+FEFF>a<U+2028>b<U+2029>"
    assert Text.visible("🏴\u{E0067}\u{E0062}\u{E007F}") == "🏴<U+E0067><U+E0062><U+E007F>"
    # Nine columns for a tag, eight for the rest, as the tab stops count.
    assert Text.visible("\u{E0067}\tx") == "<U+E0067>       x"
    assert Text.visible("\u200B\tx") == "<U+200B>        x"
  end

  # The matcher that ends a run looks for the bytes a control may start with (`C2`, `E2 80`,
  # `E2 81`, `EF BB`, `F3 A0`); a character that starts with them and is no control is text.
  test "a character that shares a control's first bytes is drawn as itself, alone and in a run" do
    for c <- [
          "\u00A0",
          "\u00A9",
          "\u00AC",
          "\u00AE",
          "\u2013",
          "\u2014",
          "\u201C",
          "\u201D",
          "\u2026",
          "\u2071",
          "\uFEFC",
          "\u{E0080}",
          "\u{E01EF}"
        ] do
      assert Text.visible(c) == c, c
      assert Text.visible("a" <> c <> "b") == "a" <> c <> "b", c
      run = String.duplicate("x" <> c, 100)
      assert Text.visible(run) == run, c
      assert Text.visible(run <> "\u200B" <> run) == run <> "<U+200B>" <> run, c
      assert Text.visible(c <> "\u2028" <> c) == c <> "<U+2028>" <> c, c
    end

    assert Text.visible("rm \u201Creport\u201D \u2013 \u00A9 Jos\u00E9\u00A0\u200Bx") ==
             "rm \u201Creport\u201D \u2013 \u00A9 Jos\u00E9\u00A0<U+200B>x"
  end

  test "a joiner is kept inside an emoji sequence and drawn anywhere else" do
    for kept <- ["👩\u200D💻", "👩🏽\u200D💻", "❤️\u200D🔥", "👨\u200D👩\u200D👧\u200D👦", "x👩\u200D💻y"] do
      assert Text.visible(kept) == kept
      refute Redoubt.Term.Cells.forbidden?(kept)
    end

    assert Text.visible("a\u200Db") == "a<U+200D>b"
    assert Text.visible("👩\u200Da") == "👩<U+200D>a"
    assert Text.visible("a\u200D💻") == "a<U+200D>💻"
    assert Text.visible("👩\u200D") == "👩<U+200D>"
    assert Text.visible("\u200D💻") == "<U+200D>💻"
    assert Text.visible("👩\u200D\u200D💻") == "👩<U+200D><U+200D>💻"
    assert Text.visible("👩́\u200D💻") == "👩́<U+200D>💻"
    assert Text.visible("क्\u200Dष") == "क्<U+200D>ष"
    # A joiner after a drawn control, a tab or a byte that is not UTF-8 is drawn too.
    assert Text.visible("\e\u200D💻") == "^[<U+200D>💻"
    assert Text.visible("\t\u200D💻") == "        <U+200D>💻"
    assert Text.visible(<<0xFF, "\u200D💻">>) == "<FF><U+200D>💻"
    # The decision counts: a joiner that is kept is a code point of the line, for the tab stops.
    assert Text.visible("👩\u200D💻\tx") == "👩\u200D💻     x"
  end

  # The one table, userland/native/cells/forbidden.txt, which the cells crate's tests hold its
  # decoder to as well. On beamlet ./test-shell mounts it beside the vectors (CELLS_VECTORS).
  defp table(kind) do
    (System.get_env("CELLS_VECTORS") || Path.expand("../../../../native/cells/vectors.json", __DIR__))
    |> Path.dirname()
    |> Path.join("forbidden.txt")
    |> File.read!()
    |> String.split("\n")
    |> Enum.filter(&String.starts_with?(&1, kind))
    |> Enum.map(fn line ->
      [^kind, first, last | _what] = String.split(line)
      String.to_integer(first, 16)..String.to_integer(last, 16)
    end)
  end

  # The cells crate's test sweeps every code point against the file; here the ranges are compared
  # as stated, and the functions probed at every range's edges, which a sweep on beamlet cannot
  # afford.
  test "the control characters and the pictographs are the shared table's, the cells crate's rule too" do
    forbidden = table("forbidden")
    assert Enum.sum(Enum.map(forbidden, &Range.size/1)) == 32 + 33 + 1 + 3 + 2 + 5 + 5 + 4 + 1 + 128
    assert Enum.sort(Text.controls()) == Enum.sort(forbidden)
    pictographic = table("pictographic")
    assert length(pictographic) == 78
    assert Enum.sort(Text.pictographs()) == Enum.sort(pictographic)
    in_any = fn ranges, c -> Enum.any?(ranges, &(c in &1)) end

    for first..last//1 <- forbidden ++ pictographic,
        c <- [first - 1, first, last, last + 1],
        c in 0..0x10FFFF do
      assert Text.control?(c) == in_any.(forbidden, c), "U+#{Integer.to_string(c, 16)}"
      assert Text.pictographic?(c) == in_any.(pictographic, c), "U+#{Integer.to_string(c, 16)}"
    end
  end

  test "the pictographs are the ones OTP's grapheme walk joins across a joiner" do
    # A code point is Extended_Pictographic exactly when OTP joins it to a pictograph and joiner
    # before it (GB11) and does not join it to a letter (so it is no mark or joiner itself).
    pictographic = fn c ->
      length(:unicode_util.gc([0x1F600, 0x200D, c])) == 1 and length(:unicode_util.gc([?a, c])) == 2
    end

    edges = for first..last//1 <- table("pictographic"), c <- [first - 1, first, last, last + 1], do: c

    for c <- edges ++ [?a, 0x200D, 0xFE0F, 0x1F3FD, 0x0301, 0x0915], c <= 0x10FFFF do
      assert Text.pictographic?(c) == pictographic.(c), "U+#{Integer.to_string(c, 16)}"
    end
  end

  test "a code point passes as itself exactly when it is not a control character, alone or in a run" do
    points =
      Enum.concat([
        0..0x3000,
        0xD7F0..0xD7FF,
        0xE000..0xE010,
        0xFEF0..0xFF00,
        0xFFF0..0x10010,
        0xDFFF0..0xE0090,
        0x10FFF0..0x10FFFF
      ])

    for c <- points do
      {text, hex} = {<<c::utf8>>, Integer.to_string(c, 16)}

      if Text.control?(c),
        do: refute(Text.visible(text) == text, hex),
        else: assert(Text.visible(text) == text, hex)

      # After a short run and after one of 100 code points (150 bytes, so the matchers' first
      # window ends before the code point); a tab's stop depends on where it is, and has its own
      # test.
      for run <- ["aé", String.duplicate("aé", 50)], c != ?\t do
        assert Text.visible(run <> text <> "z") == run <> Text.visible(text) <> "z", hex
      end
    end
  end

  # The VM's matchers take a printable run in windows of 128 bytes and then 512. At every window's
  # edge, whatever falls there (a control of one byte, of two as C2 8x or C2 AD, of three as
  # E2 80 AA or EF BB BF, or of four as a tag; a character of two, three or four bytes cut by the
  # edge; a byte that is not UTF-8, or a sequence cut short; a combining mark; a tab; an emoji
  # sequence, whose joiner is kept), the text is drawn as the matchers draw it a unit at a time: a
  # unit alone fills no window, so that path is the reference.
  test "the matchers draw a run as they draw its units one at a time, at every window's edge" do
    specials = [
      "\e",
      "\u0000",
      "\u007F",
      "\u0085",
      "\u009B",
      "\u00AD",
      "\u200B",
      "\u2028",
      "\u202A",
      "\u2060",
      "\u2069",
      "\uFEFF",
      "\u{E0001}",
      "\u{E007F}",
      "\u00A0",
      "\u00A9",
      "\u2013",
      "\u201C",
      "\u2071",
      "\uFEFC",
      "\u{E0080}",
      "\t",
      "é",
      "€",
      "🙂",
      "\u0301",
      "\u200D",
      "👩\u200D💻",
      "👩🏽\u200D💻",
      <<0xFF>>,
      <<0xE2, 0x82>>,
      <<0xF0, 0x9F, 0x99>>,
      <<0xED, 0xA0, 0x80>>,
      <<0xC0, 0x80>>,
      <<0xE0, 0x80, 0x80>>
    ]

    for filler <- ["x", "é"], at <- edges(), special <- specials do
      text = prefix(filler, at) <> special <> String.duplicate(filler, 600)
      assert Text.visible(text, 3) == by_units(text, 3), "#{inspect(special)} at byte #{at} after #{filler}"
    end

    for filler <- ["x", "é", "я", "x\u00e9", "世"], n <- [65, 80, 100, 600, 1500] do
      text = String.duplicate(filler, n)
      assert Text.visible(text) == text, "#{n} of #{filler}"
      # A multibyte sequence cut short at the text's very end.
      cut = text <> <<0xF0, 0x9F, 0x99>>
      assert Text.visible(cut, 3) == by_units(cut, 3), "#{n} of #{filler}, cut at the end"
    end
  end

  # The byte offsets about the windows' edges: the text's start, and 128, 640 and 1152 bytes in,
  # five bytes each side.
  defp edges, do: for(edge <- [0, 128, 640, 1152], at <- max(edge - 5, 0)..(edge + 5), do: at)

  # `at` bytes of `filler`, one byte of ASCII making up an odd count.
  defp prefix(filler, at) do
    String.duplicate(filler, div(at, byte_size(filler))) <> String.duplicate("x", rem(at, byte_size(filler)))
  end

  # `visible/2` a unit at a time (a code point, or a byte that is not one; an emoji sequence whole,
  # since a joiner is judged by what stands beside it), the column carried: the Erlang path's own
  # answer.
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
  defp units(<<"👩\u200D💻", rest::binary>>), do: ["👩\u200D💻" | units(rest)]
  defp units(<<"👩🏽\u200D💻", rest::binary>>), do: ["👩🏽\u200D💻" | units(rest)]
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

    every_control = for c <- Enum.concat(Text.controls()), into: <<>>, do: <<c::utf8>>

    for input <- [every_byte, every_c1, every_byte <> every_c1, every_control, "👩" <> every_control <> "💻"] do
      out = Text.visible(input)
      assert String.valid?(out)
      refute out |> String.to_charlist() |> Enum.any?(&Text.control?/1)
    end
  end
end
