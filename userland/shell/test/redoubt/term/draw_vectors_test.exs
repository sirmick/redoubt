defmodule Redoubt.Term.DrawVectorsTest do
  # The frames `Redoubt.Term.Frame.draw/1` draws, and the bytes it draws them as, written to
  # userland/otp/screen/draw.json: the screen natives' encoder (`redoubt_screen:draw/1`) is held
  # to them byte for byte by beamlet-screen's own tests, which force each frame's cells into a
  # buffer, symbols holding control characters among them, as nothing else can (the buffer refuses
  # them). `DRAW_VECTORS=write mix test test/redoubt/term/draw_vectors_test.exs` writes the file;
  # this test fails when it is not current. It runs where the file is: on the BEAM, from the tree.
  use ExUnit.Case, async: true

  import Bitwise

  alias Redoubt.Term.Frame

  @path Path.expand("../../../../otp/screen/draw.json", __DIR__)

  unless File.exists?(@path) or System.get_env("DRAW_VECTORS") == "write" do
    @moduletag skip:
                 "the screen natives' draw vectors are not under this tree (beamlet mounts only the shell)"
  end

  # Every control character `Redoubt.Term.Text` makes visible, and text that would drive a
  # terminal if it were not.
  @controls Enum.concat([0..0x1F, 0x7F..0x9F, 0x202A..0x202E, 0x2066..0x2069])
  @hostile [
    "\e]52;c;cHduZWQ=\a",
    "\e]8;;http://x\e\\",
    "\e[2J",
    "\e[6n",
    "\u009b31m",
    "\u009d0;t\u009c",
    "x\r\n",
    "a\tb",
    "\t\t",
    "<\u202E>",
    "\u2066x\u2069",
    "\x1c",
    "\x7f\x00"
  ]
  @colors [
            :reset,
            {:indexed, 0},
            {:indexed, 7},
            {:indexed, 8},
            {:indexed, 15},
            {:indexed, 16},
            {:indexed, 255}
          ] ++
            [{:rgb, 0, 0, 0}, {:rgb, 255, 128, 1}]

  defp cell(x, y, symbol, fg \\ :reset, bg \\ :reset, modifiers \\ 0),
    do: %{x: x, y: y, symbol: symbol, fg: fg, bg: bg, modifiers: modifiers}

  # One cell a row: a cell is placed afresh after a symbol of more than one grapheme, which only
  # a forced cell can be, so these are never side by side.
  defp apart(symbols, styles) do
    for {{s, i}, {fg, bg, m}} <- Enum.zip(Enum.with_index(symbols), Stream.cycle(styles)),
        do: cell(rem(i * 7, 70), i, s, fg, bg, m)
  end

  defp vectors do
    styles = for fg <- @colors, bg <- Enum.take(@colors, 3), m <- [0, 1, 511], do: {fg, bg, m}
    controls = for c <- @controls, do: <<c::utf8>>

    # Side by side along rows: every attribute alone, then all, every colour form as either,
    # a style repeated (set once), and a wide symbol, a selector's, and a joined one among them.
    row =
      Enum.map(0..8, &cell(&1, 0, "m", :reset, :reset, 1 <<< &1)) ++
        [cell(9, 0, "M", :reset, :reset, 511)] ++
        Enum.with_index(@colors, fn c, i -> cell(10 + i, 0, "f", c) end) ++
        Enum.with_index(@colors, fn c, i -> cell(20 + i, 0, "b", :reset, c) end) ++
        [cell(30, 0, "s", {:indexed, 3}, :reset, 4), cell(31, 0, "s", {:indexed, 3}, :reset, 4)] ++
        [cell(33, 0, "界"), cell(35, 0, "a"), cell(36, 0, "e\u0301"), cell(37, 0, "b")] ++
        [cell(38, 0, "❤\uFE0F"), cell(40, 0, "c"), cell(79, 0, "z"), cell(0, 1, "next row")]

    [
      {"every control character, made visible", 80, Enum.count(controls), true, apart(controls, styles)},
      {"text that would drive a terminal, made visible", 80, Enum.count(@hostile), true,
       apart(@hostile, styles)},
      {"styles, colours and widths side by side, cleared first", 80, 2, true, row},
      {"styles, colours and widths side by side, not cleared", 80, 2, false, row},
      {"nothing, cleared", 4, 1, true, []},
      {"nothing, not cleared", 4, 1, false, []}
    ]
  end

  # A frame's cells in the cell protocol's layout, without its checks: what is forced into the
  # buffer.
  defp wire(width, height, clear, cells) do
    body =
      for c <- cells, into: <<>> do
        <<c.x::little-16, c.y::little-16, byte_size(c.symbol), c.symbol::binary, color(c.fg)::binary,
          color(c.bg)::binary, c.modifiers::little-16>>
      end

    <<1, if(clear, do: 1, else: 0), width::little-16, height::little-16, length(cells)::little-32>> <> body
  end

  defp color(:reset), do: <<0, 0, 0, 0>>
  defp color({:indexed, i}), do: <<1, i, 0, 0>>
  defp color({:rgb, r, g, b}), do: <<2, r, g, b>>

  defp json do
    lines =
      for {name, w, h, clear, cells} <- vectors() do
        drawn = %{clear: clear, cells: cells} |> Frame.draw() |> IO.iodata_to_binary()

        ~s({"name": #{JSON.encode!(name)}, "width": #{w}, "height": #{h}, "frame": "#{hex(wire(w, h, clear, cells))}", "drawn": "#{hex(drawn)}"})
      end

    "[\n  " <> Enum.join(lines, ",\n  ") <> "\n]\n"
  end

  defp hex(bytes), do: Base.encode16(bytes, case: :lower)

  test "draw.json holds what Frame.draw/1 draws" do
    json = json()

    if System.get_env("DRAW_VECTORS") == "write" do
      File.write!(@path, json)
    else
      assert File.read!(@path) == json,
             "draw.json is not current: run DRAW_VECTORS=write mix test test/redoubt/term/draw_vectors_test.exs"
    end
  end
end
