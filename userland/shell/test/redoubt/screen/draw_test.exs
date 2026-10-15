defmodule Redoubt.Screen.DrawTest do
  # The screen natives' encoder (`Redoubt.Term.Buffer.draw/1`) is held to the shell's,
  # `Redoubt.Term.Frame.draw/1`, byte for byte: twin buffers drawn into alike, one drawn by the
  # natives, the other's diff decoded and drawn by `Frame`. Over every code point below U+3000
  # that a symbol may be, the planes' edges, the text the hostile-text case sends made visible,
  # every attribute and colour form, and frames that clear and that do not. The symbols holding
  # control characters, which no buffer takes, are held to it by beamlet-screen's own tests
  # (userland/otp/screen/draw.json).
  use ExUnit.Case, async: true

  alias Redoubt.Term.{Buffer, Cells, Frame, Text}

  unless Buffer.available?() do
    @moduletag skip: "the screen buffer is beamlet's natives, which the BEAM does not have"
  end

  @controls Enum.concat(Redoubt.Term.Text.controls())
  @edges [0xFFFC, 0xFFFD, 0xFFFE, 0xFFFF, 0x10000, 0x1FFFD, 0x1FFFF, 0x20000, 0x2FFFF, 0x30000] ++
           [0x3FFFF, 0xE0000, 0xE0001, 0xE007F, 0xEFFFF, 0xF0000, 0xFFFFD, 0x100000, 0x10FFFF]
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

  # Each in a process of its own, which holds its buffers until it ends (a process holds at
  # most four): what the natives drew, and what `Frame` drew of the twin's diff.
  defp both(cols, rows, fun) do
    Task.await(
      Task.async(fn ->
        [b, twin] = for _ <- 1..2, do: Buffer.new(cols, rows)
        fun.(b)
        fun.(twin)
        {:ok, frame} = Cells.decode(Buffer.diff(twin))
        {Buffer.draw(b), IO.iodata_to_binary(Frame.draw(frame))}
      end),
      60_000
    )
  end

  defp style(i),
    do: {Enum.at(@colors, rem(i, 9)), Enum.at(@colors, rem(div(i, 9), 9)), rem(i * 37, 512)}

  test "every code point below U+3000 a symbol may be, and the planes' edges, are drawn alike" do
    points = Enum.reject(Enum.to_list(0..0x2FFF) ++ @edges, &(&1 in @controls or &1 in 0xD800..0xDFFF))

    # A screen of 80 by 24 at a time; each code point a symbol of its own, side by side where it
    # lands, in a style that changes every few cells, with gaps that move the cursor.
    for chunk <- Enum.chunk_every(points, 80 * 24 - 80), clear <- [true, false] do
      {native, frame} =
        both(80, 24, fn b ->
          unless clear, do: Buffer.diff(b)

          Enum.with_index(chunk, fn c, i ->
            Buffer.put(b, rem(i, 80), div(i, 80) + rem(i, 2), <<c::utf8>>, style(div(i, 3)))
          end)
        end)

      assert native == frame, "from U+#{Integer.to_string(hd(chunk), 16)}, clear #{clear}"
    end
  end

  test "hostile text, made visible, is drawn alike, and holds no sequence but the encoder's" do
    hostile = [
      "\e]52;c;cHduZWQ=\a\e]0;pwned\a\eX",
      "\e[2J\e[6n\e]8;;http://x\e\\link\e]8;;\e\\",
      "\u009b31m\u009d0;t\u009c",
      "<\u202E>evil\u2066x\u2069",
      "a\tb\t\tc\r\n\x7f\x00\x1c",
      "e\u0301 界 ❤\uFE0F 🇫🇷 👩‍👩‍👧 X"
    ]

    {native, frame} =
      both(80, 24, fn b ->
        hostile
        |> Enum.with_index()
        |> Enum.each(fn {text, y} -> Buffer.put(b, 1, y, Text.visible(text), style(y)) end)
      end)

    assert native == frame
    refute native =~ "\e]"
    refute native =~ "\x1c"
  end

  test "frames after partial changes are drawn alike, each frame only what changed" do
    :rand.seed(:exsss, {29, 31, 37})
    symbols = ["a", "Z", "界", "e\u0301", "❤\uFE0F", "─", " ", "\u2800"]

    # The same seeded steps on both buffers: a few puts and a fill a frame, over what is there.
    steps =
      for _ <- 1..60 do
        for _ <- 1..:rand.uniform(6) do
          x = :rand.uniform(80) - 1
          y = :rand.uniform(24) - 1
          text = Enum.map_join(1..:rand.uniform(12), fn _ -> Enum.random(symbols) end)
          {x, y, text, style(:rand.uniform(200))}
        end
      end

    Task.await(
      Task.async(fn ->
        [b, twin] = for _ <- 1..2, do: Buffer.new(80, 24)

        for {frame_steps, n} <- Enum.with_index(steps) do
          for b <- [b, twin], {x, y, text, style} <- frame_steps do
            if rem(n, 7) == 3,
              do: Buffer.fill(b, {x, y, 5, 2}, "#", style),
              else: Buffer.put(b, x, y, text, style)
          end

          {:ok, frame} = Cells.decode(Buffer.diff(twin))
          assert Buffer.draw(b) == IO.iodata_to_binary(Frame.draw(frame)), "frame #{n}"
        end
      end),
      60_000
    )
  end
end
