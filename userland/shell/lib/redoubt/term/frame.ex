defmodule Redoubt.Term.Frame do
  @moduledoc """
  The encoder's half for screens: a `cells` frame, as `Redoubt.Term.Cells.decode/1` reads one,
  drawn as escape sequences (docs/userland/shell.md, "The terminal library").

  Each cell is placed absolutely, `CSI row;col H`, unless the cursor is already there after the
  cell before it; its style is set with SGR when it differs from the last one drawn (the
  attributes, and 16, 256 or 24-bit colours); its symbol is drawn through `Redoubt.Term.Text`'s
  rule, though a decoded symbol holds no control character already. After a wide symbol the
  next cell is placed absolutely again: the person's terminal may measure it otherwise, and a
  disagreement then costs one cell's place, never the rest of the row. A frame that clears the
  screen begins with `CSI 2J`; every frame ends with the style reset.

  A screen is drawn on the alternate screen with the cursor hidden (`enter/0`), and leaving it
  (`leave/0`) shows the main screen as it was.
  """

  import Bitwise

  alias Redoubt.Term.{Buffer, Text, Width}

  @csi "\e["

  @doc "Into a screen: the alternate screen, the cursor hidden."
  @spec enter() :: iodata()
  def enter, do: [@csi, "?1049h", @csi, "?25l"]

  @doc "Out of a screen: the style reset, the cursor shown, the main screen back as it was."
  @spec leave() :: iodata()
  def leave, do: [@csi, "0m", @csi, "?25h", @csi, "?1049l"]

  @doc "The escape sequences that draw `frame`."
  @spec draw(Redoubt.Term.Cells.frame()) :: iodata()
  def draw(%{clear: clear, cells: cells}) do
    {out, _cursor, _style} = Enum.reduce(cells, start(clear), &cell/2)
    [out, @csi, "0m"]
  end

  @doc """
  The escape sequences that draw the frame in `bytes`, read through the one decoder a cell at a
  time (`Redoubt.Term.Cells.reduce/3`), so a whole screen's frame is never held as a list of cells;
  or why the decoder refuses it, and then nothing.
  """
  @spec draw_bytes(binary()) :: {:ok, iodata()} | {:error, Redoubt.Term.Cells.error()}
  def draw_bytes(bytes) do
    step = fn
      {:frame, %{clear: clear}}, nil -> {:cont, start(clear)}
      {:cell, c}, acc -> {:cont, cell(c, acc)}
    end

    case Redoubt.Term.Cells.reduce(bytes, nil, step) do
      {:ok, {out, _cursor, _style}} -> {:ok, [out, @csi, "0m"]}
      {:error, _} = error -> error
    end
  end

  defp start(clear), do: {[if(clear, do: [@csi, "0m", @csi, "2J"], else: [@csi, "0m"])], nil, Buffer.plain()}

  defp cell(cell, {out, cursor, style}) do
    at = {cell.y, cell.x}

    place =
      if cursor == at,
        do: [],
        else: [@csi, Integer.to_string(cell.y + 1), ";", Integer.to_string(cell.x + 1), "H"]

    new_style = {cell.fg, cell.bg, cell.modifiers}
    set = if new_style == style, do: [], else: sgr(new_style)
    w = Width.columns(cell.symbol)
    # After a wide symbol, where the cursor is is the terminal's to say: placed again.
    next = if w == 1, do: {cell.y, cell.x + 1}, else: nil
    {[out, place, set, Text.visible(cell.symbol)], next, new_style}
  end

  # The whole style, from a reset: the attributes, then the colours.
  defp sgr({fg, bg, modifiers}) do
    codes = [0 | attributes(modifiers)] ++ color(fg, 30, 90, 38) ++ color(bg, 40, 100, 48)
    [@csi, Enum.join(codes, ";"), "m"]
  end

  # The cell protocol's modifier bit i is SGR attribute i + 1, bold to crossed out.
  defp attributes(bits), do: for(i <- 0..8, (bits >>> i &&& 1) == 1, do: i + 1)

  defp color(:reset, _base, _bright, _ext), do: []
  defp color({:indexed, i}, base, _bright, _ext) when i < 8, do: [base + i]
  defp color({:indexed, i}, _base, bright, _ext) when i < 16, do: [bright + i - 8]
  defp color({:indexed, i}, _base, _bright, ext), do: [ext, 5, i]
  defp color({:rgb, r, g, b}, _base, _bright, ext), do: [ext, 2, r, g, b]
end
