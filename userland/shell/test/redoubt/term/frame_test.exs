defmodule Redoubt.Term.FrameTest do
  use ExUnit.Case, async: true

  alias Redoubt.Term.{Cells, Frame}
  alias Redoubt.Test.Terminal

  # A frame's bytes, as a screen buffer's diff or a native program gives them, decoded by the one
  # decoder the session reads frames with.
  defp frame(width, height, clear, cells) do
    bytes =
      IO.iodata_to_binary([
        <<1, if(clear, do: 1, else: 0), width::little-16, height::little-16, length(cells)::little-32>>,
        for {x, y, symbol, fg, bg, mods} <- cells do
          [
            <<x::little-16, y::little-16, byte_size(symbol)>>,
            symbol,
            color(fg),
            color(bg),
            <<mods::little-16>>
          ]
        end
      ])

    {:ok, frame} = Cells.decode(bytes)
    frame
  end

  defp color(:reset), do: <<0, 0, 0, 0>>
  defp color({:indexed, i}), do: <<1, i, 0, 0>>
  defp color({:rgb, r, g, b}), do: <<2, r, g, b>>

  defp draw(terminal, frame), do: Terminal.feed(terminal, Frame.draw(frame))

  test "cells are drawn where the frame puts them, in their colours and attributes" do
    f =
      frame(10, 3, true, [
        {0, 0, "a", :reset, :reset, 0},
        {1, 0, "b", {:indexed, 1}, :reset, 1},
        {5, 2, "c", {:indexed, 12}, {:indexed, 200}, 8},
        {9, 2, "d", {:rgb, 1, 2, 3}, {:rgb, 4, 5, 6}, 64}
      ])

    t = draw(Terminal.new(10, 3), f)
    assert Terminal.lines(t) == ["ab", "", "     c   d"]
    assert Terminal.style(t, 0, 1).fg == {:indexed, 1}
    assert Terminal.bold?(t, 0, 1)
    refute Terminal.bold?(t, 0, 0)

    assert Terminal.style(t, 2, 5) == %{
             fg: {:indexed, 12},
             bg: {:indexed, 200},
             modifiers: MapSet.new([:underlined])
           }

    assert Terminal.style(t, 2, 9) == %{
             fg: {:rgb, 1, 2, 3},
             bg: {:rgb, 4, 5, 6},
             modifiers: MapSet.new([:reversed])
           }
  end

  test "a frame that clears the screen clears it; one that does not draws over what is there" do
    t = Terminal.new(5, 2) |> Terminal.feed("old")
    t = draw(t, frame(5, 2, false, [{0, 1, "x", :reset, :reset, 0}]))
    assert Terminal.lines(t) == ["old", "x"]
    t = draw(t, frame(5, 2, true, [{4, 1, "y", :reset, :reset, 0}]))
    assert Terminal.lines(t) == ["", "    y"]
  end

  test "a run of cells on a row is placed once, and the cell after a wide one is placed again" do
    f = frame(6, 1, false, [{0, 0, "a", :reset, :reset, 0}, {1, 0, "b", :reset, :reset, 0}])
    assert IO.iodata_to_binary(Frame.draw(f)) |> String.split("H") |> length() == 2

    f = frame(6, 1, false, [{0, 0, "世", :reset, :reset, 0}, {2, 0, "x", :reset, :reset, 0}])
    bytes = IO.iodata_to_binary(Frame.draw(f))
    assert bytes =~ "\e[1;3Hx"
    assert Terminal.lines(Terminal.feed(Terminal.new(6, 1), bytes)) == ["世x"]
  end

  test "a screen is drawn on the alternate screen, and leaving it shows the line as it was" do
    t = Terminal.new(10, 3) |> Terminal.feed("prompt> ")
    t = Terminal.feed(t, Frame.enter())
    assert Terminal.alternate?(t)
    refute t.cursor
    t = draw(t, frame(10, 3, true, [{2, 1, "#", :reset, :reset, 0}]))
    assert Terminal.lines(t) == ["", "  #"]
    t = Terminal.feed(t, Frame.leave())
    refute Terminal.alternate?(t)
    assert t.cursor
    assert Terminal.lines(t) == ["prompt>"]
    assert Terminal.cursor(t) == {0, 8}
  end

  test "a frame with a control character never reaches the encoder: the decoder refuses it" do
    bytes =
      <<1, 0, 4::little-16, 1::little-16, 1::little-32, 0::little-16, 0::little-16, 2, "\e[">> <>
        <<0::64, 0::16>>

    assert Cells.decode(bytes) == {:error, :symbol}
  end
end
