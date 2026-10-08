defmodule Redoubt.Screen.Widgets do
  @moduledoc """
  Widgets are functions, not processes (docs/userland/shell.md, "Full-screen programs"): each
  draws into a rectangle of a screen's buffer from what it is given. These are the ones `pick`
  uses: a box, a list and a status line.

  Every text a widget draws is made visible first (`Redoubt.Term.Text.visible/1`), so a control
  character in a title or an item is drawn as `^[` and the like, and fitted to its columns by
  `Redoubt.Term.Width`.
  """

  alias Redoubt.Term.{Buffer, Text, Width}

  @doc """
  A box: a border in box drawing around `rect`, a title in its top border, and a shadow one
  cell to its right and below. Options: `:title`, `:style`, `:shadow` (`true`).
  """
  @spec box(Buffer.t(), Buffer.rect(), keyword()) :: :ok
  def box(buffer, rect, opts \\ [])

  def box(buffer, {x, y, w, h}, opts) when w >= 2 and h >= 2 do
    style = Keyword.get(opts, :style, Buffer.plain())

    if Keyword.get(opts, :shadow, true) do
      shadow = Buffer.style(bg: {:indexed, 0})
      Buffer.fill(buffer, {x + w, y + 1, 1, h}, " ", shadow)
      Buffer.fill(buffer, {x + 1, y + h, w, 1}, " ", shadow)
    end

    Buffer.fill(buffer, {x + 1, y + 1, w - 2, h - 2}, " ", style)
    Buffer.fill(buffer, {x + 1, y, w - 2, 1}, "─", style)
    Buffer.fill(buffer, {x + 1, y + h - 1, w - 2, 1}, "─", style)
    Buffer.fill(buffer, {x, y + 1, 1, h - 2}, "│", style)
    Buffer.fill(buffer, {x + w - 1, y + 1, 1, h - 2}, "│", style)
    Buffer.put(buffer, x, y, "┌", style)
    Buffer.put(buffer, x + w - 1, y, "┐", style)
    Buffer.put(buffer, x, y + h - 1, "└", style)
    Buffer.put(buffer, x + w - 1, y + h - 1, "┘", style)

    case Keyword.get(opts, :title) do
      nil -> :ok
      title -> label(buffer, {x + 2, y, max(w - 4, 0), 1}, " " <> title <> " ", style)
    end

    :ok
  end

  def box(_buffer, _rect, _opts), do: :ok

  @doc """
  A list: `items` (text) a row each from `offset`, in `rect`, the one at `selected` drawn
  reversed across the whole row.
  """
  @spec list(Buffer.t(), Buffer.rect(), [String.t()], non_neg_integer(), non_neg_integer(), Buffer.style()) ::
          :ok
  def list(buffer, {x, y, w, h}, items, selected, offset, style \\ Buffer.plain()) do
    {fg, bg, bits} = style
    {_, _, reversed} = Buffer.style(reversed: true)
    chosen = {fg, bg, Bitwise.bor(bits, reversed)}

    items
    |> Enum.drop(offset)
    |> Enum.take(h)
    |> Enum.with_index(offset)
    |> Enum.each(fn {item, i} ->
      row_style = if i == selected, do: chosen, else: style
      label(buffer, {x, y + i - offset, w, 1}, " " <> item, row_style, pad: true)
    end)
  end

  @doc "A status line: `text` across `rect`'s first row, reversed."
  @spec status(Buffer.t(), Buffer.rect(), String.t()) :: :ok
  def status(buffer, {x, y, w, _h}, text) do
    label(buffer, {x, y, w, 1}, " " <> text, Buffer.style(reversed: true), pad: true)
  end

  @doc """
  `text` made visible and drawn at `rect`'s top left, cut to its width; with `pad: true` the rest
  of the row is filled with spaces in the same style.
  """
  @spec label(Buffer.t(), Buffer.rect(), String.t(), Buffer.style(), keyword()) :: :ok
  def label(buffer, {x, y, w, _h}, text, style, opts \\ []) do
    fitted = fit(Text.visible(text), w)
    written = if fitted == "", do: 0, else: Buffer.put(buffer, x, y, fitted, style)

    if Keyword.get(opts, :pad, false) and written < w do
      Buffer.fill(buffer, {x + written, y, w - written, 1}, " ", style)
    end

    :ok
  end

  @doc "The longest start of `text` that fits in `cols` columns."
  @spec fit(String.t(), non_neg_integer()) :: String.t()
  def fit(text, cols) do
    text
    |> String.graphemes()
    |> Enum.reduce_while({[], 0}, fn g, {acc, used} ->
      w = Width.grapheme(g)
      if used + w > cols, do: {:halt, {acc, used}}, else: {:cont, {[acc, g], used + w}}
    end)
    |> elem(0)
    |> IO.iodata_to_binary()
  end
end
